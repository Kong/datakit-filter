use log::info;
use log::warn;
use proxy_wasm::{traits::*, types::*};
use serde_json_wasm::de;
use std::collections::BTreeMap;
use std::mem;

mod config;
mod data;
mod dependency_graph;
mod nodes;

use crate::config::Config;
use crate::data::{Data, State, Payload};
use crate::dependency_graph::DependencyGraph;
use crate::nodes::{Node, NodeConfig};

// -----------------------------------------------------------------------------
// Root Context
// -----------------------------------------------------------------------------

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
            let mut nodes = NodeMap::new();
            let mut node_names = Vec::new();
            build_node_map(&mut nodes, &mut node_names, config);

            Some(Box::new(DataKitFilter {
                node_names,
                nodes: Some(nodes),
                // FIXME: is it possible to do lifetime annotations
                // to avoid cloning graph every time?
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
    node_names: Vec<String>,
    nodes: Option<NodeMap>,
    data: Data,
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
        body_size: usize,
        _num_trailers: usize,
    ) {
        log::info!("DataKitFilter: on http call response, id = {:?}", token_id);

        if let Some(mut nodes) = mem::take(&mut self.nodes) {
            for name in &self.node_names {
                let node: &mut Box<dyn Node> = nodes
                    .get_mut(name)
                    .expect("self.nodes doesn't match self.node_names");
                if let Some(inputs) = self.data.get_inputs_for(node.get_name(), Some(token_id)) {
                    let state = node.on_http_call_response(self, inputs, body_size);
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

impl HttpContext for DataKitFilter {
    fn on_http_request_headers(&mut self, _nheaders: usize, _eof: bool) -> Action {
        self.run_nodes()
    }

    fn on_http_request_body(&mut self, _body_size: usize, _eof: bool) -> Action {
        self.run_nodes()
    }

    fn on_http_response_headers(&mut self, _nheaders: usize, _eof: bool) -> Action {
        let action = self.run_nodes();

        if let Some(inputs) = self.data.get_inputs_for("response", None) {
            assert!(inputs.len() > 0);
            if let Some(response_body) = inputs[0] {
                if let Payload::Json(_) = response_body {
                    self.set_http_response_header("Content-Type", Some("application/json"));
                }
                self.set_http_response_header("Content-Length", response_body.len().map(|n| n.to_string()).as_deref());
                self.set_http_response_header("Content-Encoding", None);
            }
        }

        action
    }

    fn on_http_response_body(&mut self, _body_size: usize, _eof: bool) -> Action {
        let action = self.run_nodes();

        if let Some(inputs) = self.data.get_inputs_for("response", None) {
            assert!(inputs.len() > 0);
            if let Some(response_body) = inputs[0] {
                let bytes = response_body.to_bytes();
                self.set_http_response_body(0, bytes.len(), &bytes);
            }
        }

        action
    }
}

proxy_wasm::main! {{
    nodes::register_node("template", nodes::template::TemplateConfig::from_map, nodes::template::Template::new_box);
    nodes::register_node("call", nodes::call::CallConfig::from_map, nodes::call::Call::new_box);

    proxy_wasm::set_log_level(LogLevel::Debug);
    proxy_wasm::set_root_context(|_| -> Box<dyn RootContext> {
        Box::new(DataKitFilterRootContext {
            config: None,
            graph: DependencyGraph::new(),
        })
    });
}}
