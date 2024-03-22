use crate::nodes::Node;
use core::slice::Iter;
use serde::Deserialize;

#[derive(Deserialize, Clone)]
pub struct Config {
    nodes: Vec<Box<dyn Node>>,
}

impl Config {
    /// An iterator of mutable references to Nodes
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut Box<dyn Node>> {
        self.nodes.iter_mut()
    }

    /// An iterator of immutable references to Nodes
    pub fn each_node(&self) -> Iter<Box<dyn Node>> {
        self.nodes.iter()
    }
}
