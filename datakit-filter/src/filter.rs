use log::info;
use log::warn;
use proxy_wasm::{traits::*, types::*};
use serde_json_wasm::de;
use std::mem;

mod config;
mod data;
mod dependency_graph;
mod nodes;

use crate::config::Config;
use crate::data::{Data, State};
use crate::dependency_graph::DependencyGraph;
use crate::nodes::Node;

// -----------------------------------------------------------------------------
// Root Context
// -----------------------------------------------------------------------------

struct DataKitFilterRootContext {
    config: Option<Config>,
    graph: DependencyGraph,
}

impl Context for DataKitFilterRootContext {}

fn populate_dependency_graph(graph: &mut DependencyGraph, config: &Config) {
    for node in config.each_node() {
        let conns = node.get_connections();
        for input in conns.each_input() {
            graph.add(input, conns.get_name());
        }
        for output in conns.each_output() {
            graph.add(conns.get_name(), output);
        }
    }
}

impl RootContext for DataKitFilterRootContext {
    fn on_configure(&mut self, _config_size: usize) -> bool {
        if let Some(config_bytes) = self.get_plugin_configuration() {
            match de::from_slice::<Config>(&config_bytes) {
                Ok(config) => {
                    populate_dependency_graph(&mut self.graph, &config);
                    self.config = Some(config);

                    true
                }
                Err(err) => {
                    warn!(
                        "on_configure: failed parsing configuration: {}: {}",
                        String::from_utf8(config_bytes).unwrap(),
                        err
                    );

                    false
                }
            }
        } else {
            warn!("on_configure: failed getting configuration");

            false
        }
    }

    fn get_type(&self) -> Option<ContextType> {
        Some(ContextType::HttpContext)
    }

    fn create_http_context(&self, context_id: u32) -> Option<Box<dyn HttpContext>> {
        info!("Context id: {}", context_id);
        if let Some(config) = &self.config {
            Some(Box::new(DataKitFilter {
                // FIXME: do we need to clone the config every time?
                // we should probably have a separate per-request data structure
                // for the nodes and keep Config as a read-only struct,
                // but right now Config is just the "holder of Nodes"
                config: Some(config.clone()),
                data: Data::new(self.graph.clone()),
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
    config: Option<Config>,
    data: Data,
}

impl DataKitFilter {
    fn run_nodes(&mut self) -> Action {
        let mut ret = Action::Continue;

        if let Some(mut config) = mem::take(&mut self.config) {
            loop {
                let mut any_ran = false;
                for node in config.iter_mut() {
                    if let Some(inputs) = self.data.get_inputs_for(node.get_name(), None) {
                        any_ran = true;
                        let state = node.run(self, inputs);
                        if let State::Waiting(_) = state {
                            ret = Action::Pause;
                        }
                        self.data.set(node.get_name(), state);
                    }
                }
                if !any_ran {
                    break;
                }
            }
            
            let _ = mem::replace(&mut self.config, Some(config));
        }

        ret
    }
}

impl Context for DataKitFilter {
    fn on_http_call_response(
        &mut self,
        token_id: u32,
        _nheaders: usize,
        body_size: usize,
        _num_trailers: usize,
    ) {
        log::info!("DataKitFilter: on http call response, id = {:?}", token_id);

        if let Some(mut config) = mem::take(&mut self.config) {
            for node in config.iter_mut() {
                if let Some(inputs) = self.data.get_inputs_for(node.get_name(), Some(token_id)) {
                    let state = node.on_http_call_response(self, inputs, body_size);
                    self.data.set(node.get_name(), state);
                    break;
                }
            }
            let _ = mem::replace(&mut self.config, Some(config));
        }
        self.run_nodes();

        self.resume_http_request();
    }
}

impl HttpContext for DataKitFilter {

    fn on_http_request_headers(&mut self, nheaders: usize, eof: bool) -> Action {
        self.run_nodes()
    }

    fn on_http_request_body(&mut self, body_size: usize, eof: bool) -> Action {
        self.run_nodes()
    }
}

proxy_wasm::main! {{
    nodes::register_node("template", nodes::template::Template::from_map);
    nodes::register_node("call", nodes::call::Call::from_map);

    proxy_wasm::set_log_level(LogLevel::Debug);
    proxy_wasm::set_root_context(|_| -> Box<dyn RootContext> {
        Box::new(DataKitFilterRootContext {
            config: None,
            graph: DependencyGraph::new(),
        })
    });
}}
