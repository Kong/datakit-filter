use crate::nodes::NodeConfig;
use core::slice::Iter;
use serde::Deserialize;

#[derive(Deserialize)]
pub struct Config {
    nodes: Vec<Box<dyn NodeConfig>>,
}

impl Config {
    /// An iterator of immutable references to Nodes
    pub fn each_node_config(&self) -> Iter<Box<dyn NodeConfig>> {
        self.nodes.iter()
    }
}
