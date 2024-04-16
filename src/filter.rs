use proxy_wasm::{traits::*, types::*};
use std::rc::Rc;

mod config;
mod data;
mod debug;
mod dependency_graph;
mod nodes;

use crate::config::Config;
use crate::data::{Data, Payload, State};
use crate::debug::{Debug, RunMode};
use crate::dependency_graph::DependencyGraph;
use crate::nodes::{Node, NodeMap};

// -----------------------------------------------------------------------------
// Root Context
// -----------------------------------------------------------------------------

struct DataKitFilterRootContext {
    config: Option<Rc<Config>>,
}

impl Context for DataKitFilterRootContext {}

impl RootContext for DataKitFilterRootContext {
    fn on_configure(&mut self, _config_size: usize) -> bool {
        match self.get_plugin_configuration() {
            Some(config_bytes) => match Config::new(config_bytes) {
                Ok(config) => {
                    self.config = Some(Rc::new(config));
                    true
                }
                Err(err) => {
                    log::warn!("on_configure: {err}");
                    false
                }
            },
            None => {
                log::warn!("on_configure: failed getting configuration");
                false
            }
        }
    }

    fn get_type(&self) -> Option<ContextType> {
        Some(ContextType::HttpContext)
    }

    fn create_http_context(&self, context_id: u32) -> Option<Box<dyn HttpContext>> {
        log::debug!("DataKitFilterRootContext: create http context id: {context_id}");

        let config = self.config.clone()?;

        let nodes = config.build_nodes();
        let graph = config.get_graph();
        let debug = config.debug().then(|| Debug::new(&config));

        // FIXME: is it possible to do lifetime annotations
        // to avoid cloning every time?
        let data = Data::new(graph.clone());

        let do_request_headers = graph.has_dependents("request_headers");
        let do_request_body = graph.has_dependents("request_body");
        let do_service_request_headers = graph.has_providers("service_request_headers");
        let do_service_request_body = graph.has_providers("service_request_body");
        let do_service_response_headers = graph.has_dependents("service_response_headers");
        let do_service_response_body = graph.has_dependents("service_response_body");
        let do_response_headers = graph.has_providers("response_headers");
        let do_response_body = graph.has_providers("response_body");

        Some(Box::new(DataKitFilter {
            config,
            nodes,
            debug,
            data,
            do_request_headers,
            do_request_body,
            do_service_request_headers,
            do_service_request_body,
            do_service_response_headers,
            do_service_response_body,
            do_response_headers,
            do_response_body,
        }))
    }
}

// -----------------------------------------------------------------------------
// Filter Context
// -----------------------------------------------------------------------------

pub struct DataKitFilter {
    config: Rc<Config>,
    nodes: NodeMap,
    data: Data,
    debug: Option<Debug>,
    do_request_headers: bool,
    do_request_body: bool,
    do_service_request_headers: bool,
    do_service_request_body: bool,
    do_service_response_headers: bool,
    do_service_response_body: bool,
    do_response_headers: bool,
    do_response_body: bool,
}

fn header_to_bool(header_value: &Option<String>) -> bool {
    match header_value {
        Some(val) => val != "off" && val != "false" && val != "0",
        None => false,
    }
}

impl DataKitFilter {
    fn debug_init(&mut self) {
        let trace_header = &self.get_http_request_header("X-DataKit-Debug-Trace");
        if header_to_bool(trace_header) {
            if let Some(ref mut debug) = self.debug {
                debug.set_tracing(true);
            }
            self.do_response_body = true;
        }
    }

    fn debug_done_headers(&mut self) {
        let ct = self.get_http_response_header("Content-Type");
        if let Some(ref mut debug) = self.debug {
            if debug.is_tracing() {
                debug.save_response_body_content_type(ct);
                self.set_http_response_header("Content-Type", Some("application/json"));
                self.set_http_response_header("Content-Length", None);
                self.set_http_response_header("Content-Encoding", None);
            }
        }
    }

    fn debug_done(&mut self) {
        if let Some(ref mut debug) = self.debug {
            if debug.is_tracing() {
                let trace = debug.get_trace();
                let bytes = trace.as_bytes();
                self.set_http_response_body(0, bytes.len(), bytes);
            }
        }
    }

    fn set_data(&mut self, name: &str, state: State) {
        if let Some(ref mut debug) = self.debug {
            debug.set_data(name, &state);
        }
        self.data.set(name, state);
    }

    fn set_headers_data(&mut self, vec: Vec<(String, String)>, name: &str) {
        let payload = data::from_pwm_headers(vec);
        self.set_data(name, State::Done(Some(payload)));
    }

    fn run_nodes(&mut self) -> Action {
        let mut ret = Action::Continue;

        loop {
            let mut any_ran = false;
            for name in self.config.get_node_names() {
                let node: &dyn Node = self
                    .nodes
                    .get(name)
                    .expect("self.nodes doesn't match self.node_names")
                    .as_ref();
                if let Some(inputs) = self.data.get_inputs_for(name, None) {
                    any_ran = true;

                    let state = node.run(self as &dyn HttpContext, &inputs);

                    if let Some(ref mut debug) = self.debug {
                        debug.run(name, &inputs, &state, RunMode::Run);
                    }

                    if let State::Waiting(_) = state {
                        ret = Action::Pause;
                    }
                    self.data.set(name, state);
                }
            }
            if !any_ran {
                break;
            }
        }

        ret
    }
}

impl Context for DataKitFilter {
    fn on_http_call_response(
        &mut self,
        token_id: u32,
        _nheaders: usize,
        _body_size: usize,
        _num_trailers: usize,
    ) {
        log::debug!("DataKitFilter: on http call response, id = {:?}", token_id);

        for name in self.config.get_node_names() {
            let node: &dyn Node = self
                .nodes
                .get(name)
                .expect("self.nodes doesn't match self.node_names")
                .as_ref();
            if let Some(inputs) = self.data.get_inputs_for(name, Some(token_id)) {
                let state = node.resume(self, &inputs);

                if let Some(ref mut debug) = self.debug {
                    debug.run(name, &inputs, &state, RunMode::Resume);
                }

                self.data.set(name, state);
                break;
            }
        }

        self.run_nodes();

        self.resume_http_request();
    }
}

impl HttpContext for DataKitFilter {
    fn on_http_request_headers(&mut self, _nheaders: usize, _eof: bool) -> Action {
        if self.debug.is_some() {
            self.debug_init()
        }

        if self.do_request_headers {
            let vec = self.get_http_request_headers();
            self.set_headers_data(vec, "request_headers");
        }

        self.run_nodes()
    }

    fn on_http_request_body(&mut self, body_size: usize, eof: bool) -> Action {
        if eof && self.do_request_body {
            if let Some(bytes) = self.get_http_request_body(0, body_size) {
                let content_type = self.get_http_request_header("Content-Type");
                let body_payload = Payload::from_bytes(bytes, content_type.as_deref());
                self.set_data("request_body", State::Done(body_payload));
            }
        }

        let action = self.run_nodes();

        if self.do_service_request_headers {
            if let Some(payload) = self.data.first_input_for("service_request_headers", None) {
                let headers = data::to_pwm_headers(Some(payload));
                self.set_http_request_headers(headers);
            }
        }

        if self.do_service_request_body {
            if let Some(payload) = self.data.first_input_for("service_request_body", None) {
                if let Ok(bytes) = payload.to_bytes() {
                    self.set_http_request_body(0, bytes.len(), &bytes);
                }
            }
        }

        action
    }

    fn on_http_response_headers(&mut self, _nheaders: usize, _eof: bool) -> Action {
        if self.do_service_response_headers {
            let vec = self.get_http_response_headers();
            self.set_headers_data(vec, "service_response_headers");
        }

        let action = self.run_nodes();

        if self.do_response_headers {
            if let Some(payload) = self.data.first_input_for("response_headers", None) {
                let headers = data::to_pwm_headers(Some(payload));
                self.set_http_response_headers(headers);
            }
        }

        if self.do_response_body {
            if let Some(payload) = self.data.first_input_for("response_body", None) {
                let content_length = payload.len().map(|n| n.to_string());
                self.set_http_response_header("Content-Length", content_length.as_deref());
                self.set_http_response_header("Content-Encoding", None);
                self.set_http_response_header("Content-Type", payload.content_type());
            }
        }

        if self.debug.is_some() {
            self.debug_done_headers()
        }

        action
    }

    fn on_http_response_body(&mut self, body_size: usize, eof: bool) -> Action {
        if !eof {
            return Action::Pause;
        }

        if eof && self.do_service_response_body {
            if let Some(bytes) = self.get_http_response_body(0, body_size) {
                let content_type = self.get_http_response_header("Content-Type");
                let payload = Payload::from_bytes(bytes, content_type.as_deref());
                self.set_data("service_response_body", State::Done(payload));
            }
        }

        let action = self.run_nodes();

        if self.do_response_body {
            if let Some(payload) = self.data.first_input_for("response_body", None) {
                if let Ok(bytes) = payload.to_bytes() {
                    self.set_http_response_body(0, bytes.len(), &bytes);
                } else {
                    self.set_http_response_body(0, 0, &[]);
                }
            } else if let Some(debug) = &self.debug {
                if let Some(bytes) = self.get_http_response_body(0, body_size) {
                    let content_type = debug.response_body_content_type();
                    let payload = Payload::from_bytes(bytes, content_type.as_deref());
                    self.set_data("response_body", State::Done(payload));
                }
            }
        }

        if self.debug.is_some() {
            self.debug_done()
        }

        action
    }
}

proxy_wasm::main! {{
    nodes::register_node("template", Box::new(nodes::template::TemplateFactory {}));
    nodes::register_node("call", Box::new(nodes::call::CallFactory {}));
    nodes::register_node("response", Box::new(nodes::response::ResponseFactory {}));
    nodes::register_node("jq", Box::new(nodes::jq::JqFactory {}));

    proxy_wasm::set_log_level(LogLevel::Debug);
    proxy_wasm::set_root_context(|_| -> Box<dyn RootContext> {
        Box::new(DataKitFilterRootContext {
            config: None,
        })
    });
}}
