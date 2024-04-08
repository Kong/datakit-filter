use proxy_wasm::traits::*;
use serde_json::Value;
use std::any::Any;
use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock};

use crate::data::{Payload, State, State::*};

pub mod call;
pub mod response;
pub mod template;

pub type NodeMap = BTreeMap<String, Box<dyn Node>>;

pub trait Node {
    fn run(&mut self, _ctx: &dyn HttpContext, _inputs: Vec<Option<&Payload>>) -> State {
        Done(None)
    }

    fn resume(&mut self, _ctx: &dyn HttpContext, _inputs: Vec<Option<&Payload>>) -> State {
        Done(None)
    }

    fn is_waiting_on(&self, _token_id: u32) -> bool {
        false
    }
}

pub trait NodeConfig {
    fn as_any(&self) -> &dyn Any;
}

pub trait NodeFactory: Send {
    fn new_config(
        &self,
        name: &str,
        inputs: &[String],
        bt: &BTreeMap<String, Value>,
    ) -> Result<Box<dyn NodeConfig>, String>;

    fn new_node(&self, config: &dyn NodeConfig) -> Box<dyn Node>;
}

type NodeTypeMap = BTreeMap<String, Box<dyn NodeFactory>>;

fn node_types() -> &'static Mutex<NodeTypeMap> {
    static NODE_TYPES: OnceLock<Mutex<NodeTypeMap>> = OnceLock::new();
    NODE_TYPES.get_or_init(|| Mutex::new(BTreeMap::new()))
}

pub fn register_node(name: &str, factory: Box<dyn NodeFactory>) -> bool {
    node_types()
        .lock()
        .unwrap()
        .insert(String::from(name), factory);
    true
}

pub fn new_config(
    node_type: &str,
    name: &str,
    inputs: &[String],
    bt: &BTreeMap<String, Value>,
) -> Result<Box<dyn NodeConfig>, String> {
    if let Some(nf) = node_types().lock().unwrap().get(node_type) {
        nf.new_config(name, inputs, bt)
    } else {
        Err(format!("no such node type: {}", node_type))
    }
}

pub fn new_node(node_type: &str, config: &dyn NodeConfig) -> Result<Box<dyn Node>, String> {
    if let Some(nf) = node_types().lock().unwrap().get(node_type) {
        Ok(nf.new_node(config))
    } else {
        Err(format!("no such node type: {}", node_type))
    }
}
