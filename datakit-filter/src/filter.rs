use lazy_static::lazy_static;
use proxy_wasm::{traits::*, types::*};
use serde_json_wasm::de;
use std::collections::BTreeMap;
use std::collections::HashSet;
use std::mem;

mod config;
mod data;
mod dependency_graph;
mod nodes;

use crate::config::Config;
use crate::data::{Data, Payload, State};
use crate::dependency_graph::DependencyGraph;
use crate::nodes::{Node, NodeConfig};

// -----------------------------------------------------------------------------
// Root Context
// -----------------------------------------------------------------------------

lazy_static! {
    static ref RESERVED_NODE_NAMES: HashSet<&'static str> = [
        "request_headers",
        "request_body",
        "service_request_headers",
        "service_request_body",
        "service_response_headers",
        "service_response_body",
        "response_headers",
        "response_body",
    ]
    .iter()
    .copied()
    .collect();
}

type NodeMap = BTreeMap<String, Box<dyn Node>>;

struct DataKitFilterRootContext {
    config: Option<Config>,
    graph: DependencyGraph,
}

impl Context for DataKitFilterRootContext {}

fn populate_dependency_graph(graph: &mut DependencyGraph, config: &Config) {
    for node_config in config.each_node_config() {
        let conns = node_config.get_connections();
        for input in conns.each_input() {
            graph.add(input, conns.get_name());
        }
        for output in conns.each_output() {
            graph.add(conns.get_name(), output);
        }
    }
}

fn build_node_map(nodes: &mut NodeMap, node_names: &mut Vec<String>, config: &Config) {
    for node_config in config.each_node_config() {
        let name = node_config.get_connections().get_name();
        match nodes::new_node(node_config) {
            Ok(node) => {
                nodes.insert(name.to_string(), node);
                node_names.push(name.to_string());
            }
            Err(err) => {
                log::error!("{}", err);
            }
        }
    }
}

impl RootContext for DataKitFilterRootContext {
    fn on_configure(&mut self, _config_size: usize) -> bool {
        if let Some(config_bytes) = self.get_plugin_configuration() {
            match de::from_slice::<Config>(&config_bytes) {
                Ok(config) => {
                    for node_config in config.each_node_config() {
                        let name = node_config.get_connections().get_name();
                        if RESERVED_NODE_NAMES.contains(name) {
                            log::warn!("on_configure: cannot use reserved node name '{}'", name);

                            return false;
                        }
                    }

                    // TODO ensure that input- and output-only restrictions for the
                    // implicit nodes are respected.
                    populate_dependency_graph(&mut self.graph, &config);

                    self.config = Some(config);

                    true
                }
                Err(err) => {
                    log::warn!(
                        "on_configure: failed parsing configuration: {}: {}",
                        String::from_utf8(config_bytes).unwrap(),
                        err
                    );

                    false
                }
            }
        } else {
            log::warn!("on_configure: failed getting configuration");

            false
        }
    }

    fn get_type(&self) -> Option<ContextType> {
        Some(ContextType::HttpContext)
    }

    fn create_http_context(&self, context_id: u32) -> Option<Box<dyn HttpContext>> {
        log::debug!(
            "DataKitFilterRootContext: create http context id: {}",
            context_id
        );

        if let Some(config) = &self.config {
            let mut nodes = NodeMap::new();
            let mut node_names = Vec::new();
            build_node_map(&mut nodes, &mut node_names, config);

            Some(Box::new(DataKitFilter {
                node_names,
                nodes: Some(nodes),
                // FIXME: is it possible to do lifetime annotations
                // to avoid cloning graph every time?
                data: Data::new(self.graph.clone()),

                do_request_headers: self.graph.has_dependents("request_headers"),
                do_request_body: self.graph.has_dependents("request_body"),
                do_service_request_headers: self.graph.has_providers("service_request_headers"),
                do_service_request_body: self.graph.has_providers("service_request_body"),
                do_service_response_headers: self.graph.has_dependents("service_response_headers"),
                do_service_response_body: self.graph.has_dependents("service_response_body"),
                do_response_headers: self.graph.has_providers("response_headers"),
                do_response_body: self.graph.has_providers("response_body"),
            }))
        } else {
            None
        }
    }
}

// -----------------------------------------------------------------------------
// Filter Context
// -----------------------------------------------------------------------------

pub struct DataKitFilter {
    node_names: Vec<String>,
    nodes: Option<NodeMap>,
    data: Data,
    do_request_headers: bool,
    do_request_body: bool,
    do_service_request_headers: bool,
    do_service_request_body: bool,
    do_service_response_headers: bool,
    do_service_response_body: bool,
    do_response_headers: bool,
    do_response_body: bool,
}

impl DataKitFilter {
    fn run_nodes(&mut self) -> Action {
        let mut ret = Action::Continue;

        if let Some(mut nodes) = mem::take(&mut self.nodes) {
            loop {
                let mut any_ran = false;
                for name in &self.node_names {
                    let node: &mut Box<dyn Node> = nodes
                        .get_mut(name)
                        .expect("self.nodes doesn't match self.node_names");
                    if let Some(inputs) = self.data.get_inputs_for(name, None) {
                        any_ran = true;
                        let state = node.run(self, inputs);
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

            let _ = mem::replace(&mut self.nodes, Some(nodes));
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

        if let Some(mut nodes) = mem::take(&mut self.nodes) {
            for name in &self.node_names {
                let node: &mut Box<dyn Node> = nodes
                    .get_mut(name)
                    .expect("self.nodes doesn't match self.node_names");
                if let Some(inputs) = self.data.get_inputs_for(node.get_name(), Some(token_id)) {
                    let state = node.resume(self, inputs);
                    self.data.set(name, state);
                    break;
                }
            }
            let _ = mem::replace(&mut self.nodes, Some(nodes));
        }
        self.run_nodes();

        self.resume_http_request();
    }
}

fn vec_of_pairs_to_map_of_vecs<V>(vec: Vec<(String, V)>) -> BTreeMap<String, Vec<V>> {
    let mut map = BTreeMap::<String, Vec<V>>::new();
    for (k, v) in vec {
        let lk = k.to_lowercase();
        match map.get_mut(&lk) {
            Some(vs) => {
                vs.push(v);
            }
            None => {
                map.insert(lk, vec![v]);
            }
        };
    }
    map
}

fn set_headers_node(data: &mut Data, vec: Vec<(String, String)>, name: &str) {
    let map = vec_of_pairs_to_map_of_vecs(vec);
    let value = serde_json::to_value(map).expect("serializable map");
    let payload = Payload::Json(value);
    data.set(name, State::Done(Some(payload)));
}

impl HttpContext for DataKitFilter {
    fn on_http_request_headers(&mut self, _nheaders: usize, _eof: bool) -> Action {
        if self.do_request_headers {
            let vec = self.get_http_request_headers();
            set_headers_node(&mut self.data, vec, "request_headers");
        }

        self.run_nodes()
    }

    fn on_http_request_body(&mut self, body_size: usize, eof: bool) -> Action {
        if eof && self.do_request_body {
            if let Some(bytes) = self.get_http_request_body(0, body_size) {
                let content_type = self.get_http_request_header("Content-Type");
                let body_payload = Payload::from_bytes(bytes, content_type.as_deref());
                self.data.set("request_body", State::Done(body_payload));
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
            if let Some(inputs) = self.data.get_inputs_for("service_request_body", None) {
                assert!(!inputs.is_empty());
                if let Some(body) = inputs[0] {
                    let bytes = body.to_bytes();
                    self.set_http_request_body(0, bytes.len(), &bytes);
                }
            }
        }

        action
    }

    fn on_http_response_headers(&mut self, _nheaders: usize, _eof: bool) -> Action {
        if self.do_service_response_headers {
            let vec = self.get_http_response_headers();
            set_headers_node(&mut self.data, vec, "service_response_headers");
        }

        let action = self.run_nodes();

        if self.do_response_headers {
            if let Some(payload) = self.data.first_input_for("response_headers", None) {
                let headers = data::to_pwm_headers(Some(payload));
                self.set_http_response_headers(headers);
            }
        }

        if self.do_response_body {
            if let Some(inputs) = self.data.get_inputs_for("response_body", None) {
                assert!(!inputs.is_empty());
                if let Some(body) = inputs[0] {
                    if let Payload::Json(_) = body {
                        self.set_http_response_header("Content-Type", Some("application/json"));
                    }
                    let content_length = body.len().map(|n| n.to_string());
                    self.set_http_response_header("Content-Length", content_length.as_deref());
                    self.set_http_response_header("Content-Encoding", None);
                }
            }
        }

        action
    }

    fn on_http_response_body(&mut self, body_size: usize, eof: bool) -> Action {
        if eof && self.do_service_response_body {
            if let Some(bytes) = self.get_http_response_body(0, body_size) {
                let content_type = self.get_http_response_header("Content-Type");
                let payload = Payload::from_bytes(bytes, content_type.as_deref());
                self.data.set("service_response_body", State::Done(payload));
            }
        }

        let action = self.run_nodes();

        if self.do_response_body {
            if let Some(inputs) = self.data.get_inputs_for("response_body", None) {
                assert!(!inputs.is_empty());
                if let Some(body) = inputs[0] {
                    let bytes = body.to_bytes();
                    self.set_http_response_body(0, bytes.len(), &bytes);
                }
            }
        }

        action
    }
}

proxy_wasm::main! {{
    nodes::register_node("template", nodes::template::TemplateConfig::from_map, nodes::template::Template::new_box);
    nodes::register_node("call", nodes::call::CallConfig::from_map, nodes::call::Call::new_box);
    nodes::register_node("response", nodes::response::ResponseConfig::from_map, nodes::response::Response::new_box);

    proxy_wasm::set_log_level(LogLevel::Debug);
    proxy_wasm::set_root_context(|_| -> Box<dyn RootContext> {
        Box::new(DataKitFilterRootContext {
            config: None,
            graph: DependencyGraph::new(),
        })
    });
}}
