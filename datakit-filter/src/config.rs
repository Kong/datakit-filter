use crate::nodes::NodeConfig;
use crate::DependencyGraph;
use core::slice::Iter;
use lazy_static::lazy_static;
use serde::Deserialize;
use serde_json_wasm::de;
use std::collections::HashSet;

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

#[derive(Deserialize, Default)]
pub struct UserConfig {
    nodes: Vec<Box<dyn NodeConfig>>,
}

pub struct Config {
    nodes: Vec<Box<dyn NodeConfig>>,
    graph: DependencyGraph,
}

impl Config {
    pub fn new(config_bytes: Vec<u8>) -> Result<Config, String> {
        match de::from_slice::<UserConfig>(&config_bytes) {
            Ok(user_config) => {
                for node_config in &user_config.nodes {
                    let name = node_config.get_name();
                    if RESERVED_NODE_NAMES.contains(name) {
                        return Err(format!("cannot use reserved node name '{}'", name));
                    }
                }

                let mut config = Config {
                    nodes: user_config.nodes,
                    graph: Default::default(),
                };

                config.populate_dependency_graph();

                Ok(config)
            }
            Err(err) => Err(format!(
                "failed parsing configuration: {}: {}",
                String::from_utf8(config_bytes).unwrap(),
                err
            )),
        }
    }

    /// An iterator of immutable references to Nodes
    pub fn each_node_config(&self) -> Iter<Box<dyn NodeConfig>> {
        self.nodes.iter()
    }

    pub fn get_graph(&self) -> &DependencyGraph {
        &self.graph
    }

    fn populate_dependency_graph(&mut self) {
        // TODO ensure that input- and output-only restrictions for the
        // implicit nodes are respected.

        for node_config in &self.nodes {
            let conns = node_config.get_connections();
            for input in conns.each_input() {
                self.graph.add(input, conns.get_name());
            }
            for output in conns.each_output() {
                self.graph.add(conns.get_name(), output);
            }
        }
    }
}
