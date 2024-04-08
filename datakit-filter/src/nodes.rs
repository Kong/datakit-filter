use core::slice::Iter;
use proxy_wasm::traits::*;
use serde::de::{Error, MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use std::any::Any;
use std::collections::BTreeMap;
use std::fmt;
use std::sync::{Mutex, OnceLock};

use crate::data::{Payload, State, State::*};

pub mod call;
pub mod template;
pub mod response;

pub trait Node {
    #[allow(clippy::borrowed_box)]
    fn new_box(config: &Box<dyn NodeConfig>) -> Box<dyn Node>
    where
        Self: Sized;

    fn run(&mut self, _ctx: &dyn HttpContext, _inputs: Vec<Option<&Payload>>) -> State {
        Done(None)
    }

    fn resume(&mut self, _ctx: &dyn HttpContext, _inputs: Vec<Option<&Payload>>) -> State {
        Done(None)
    }

    fn is_waiting_on(&self, _token_id: u32) -> bool {
        false
    }

    fn get_name(&self) -> &str;
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Connections {
    name: String,
    inputs: Vec<String>,
    outputs: Vec<String>,
}

impl Connections {
    pub fn each_input(&self) -> Iter<String> {
        self.inputs.iter()
    }

    pub fn each_output(&self) -> Iter<String> {
        self.outputs.iter()
    }

    pub fn get_name(&self) -> &str {
        &self.name
    }
}

pub trait NodeConfig {
    fn as_any(&self) -> &dyn Any;

    fn get_node_type(&self) -> &'static str;

    fn get_connections(&self) -> &Connections;

    fn clone_dyn(&self) -> Box<dyn NodeConfig>;

    fn from_map(bt: BTreeMap<String, Value>, connections: Connections) -> Box<dyn NodeConfig>
    where
        Self: Sized;
}

type NodeConfigFromMapFn = fn(BTreeMap<String, Value>, Connections) -> Box<dyn NodeConfig>;
type NodeNewFn = fn(&Box<dyn NodeConfig>) -> Box<dyn Node>;

struct NodeFactory {
    from_map: NodeConfigFromMapFn,
    new: NodeNewFn,
}

type NodeTypeMap = BTreeMap<String, NodeFactory>;

fn node_types() -> &'static Mutex<NodeTypeMap> {
    static NODE_TYPES: OnceLock<Mutex<NodeTypeMap>> = OnceLock::new();
    NODE_TYPES.get_or_init(|| Mutex::new(BTreeMap::new()))
}

pub fn register_node(name: &str, from_map: NodeConfigFromMapFn, new: NodeNewFn) -> bool {
    node_types()
        .lock()
        .unwrap()
        .insert(String::from(name), NodeFactory { from_map, new });
    true
}

#[allow(clippy::borrowed_box)]
pub fn new_node(config: &Box<dyn NodeConfig>) -> Result<Box<dyn Node>, String> {
    let node_type = config.get_node_type();
    if let Some(nf) = node_types().lock().unwrap().get(node_type) {
        Ok((nf.new)(config))
    } else {
        Err(format!("no such node type: {}", node_type))
    }
}

impl<'a> Deserialize<'a> for Box<dyn NodeConfig> {
    fn deserialize<D>(de: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'a>,
    {
        struct DynNodeConfigVisitor;

        impl<'de> Visitor<'de> for DynNodeConfigVisitor {
            type Value = Box<dyn NodeConfig>;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a DataKit node config")
            }

            fn visit_map<M>(self, mut map: M) -> Result<Self::Value, M::Error>
            where
                M: MapAccess<'de>,
            {
                let mut bt = BTreeMap::new();
                let mut typ: Option<String> = None;
                let mut connections: Connections = Default::default();
                let mut has_name = false;
                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "type" => {
                            if let Ok(serde_json::Value::String(value)) = map.next_value() {
                                typ = Some(value);
                            }
                        }
                        "name" => {
                            if let Ok(serde_json::Value::String(value)) = map.next_value() {
                                connections.name = value;
                                has_name = true;
                            }
                        }
                        "input" => {
                            if let Ok(serde_json::Value::String(value)) = map.next_value() {
                                connections.inputs.push(value);
                            }
                        }
                        "inputs" => {
                            if let Ok(values) = map.next_value() {
                                if let Ok(inputs) = serde_json::from_value::<Vec<String>>(values) {
                                    connections.inputs = inputs;
                                }
                            }
                        }
                        "output" => {
                            if let Ok(serde_json::Value::String(value)) = map.next_value() {
                                connections.outputs.push(value);
                            }
                        }
                        "outputs" => {
                            if let Ok(values) = map.next_value() {
                                if let Ok(outputs) = serde_json::from_value::<Vec<String>>(values) {
                                    connections.outputs = outputs;
                                }
                            }
                        }
                        _ => {
                            if let Ok(value) = map.next_value() {
                                bt.insert(key, value);
                            }
                        }
                    }
                }

                if !has_name {
                    connections.name = format!("{:p}", &connections);
                }

                if let Some(t) = typ {
                    if let Some(nf) = node_types().lock().unwrap().get(&t) {
                        let v: Self::Value = (nf.from_map)(bt, connections);

                        Ok(v)
                    } else {
                        Err(Error::unknown_variant(&t, &[]))
                    }
                } else {
                    Err(Error::missing_field("type"))
                }
            }
        }

        de.deserialize_map(DynNodeConfigVisitor)
    }
}

pub fn get_config_value<T: for<'de> serde::Deserialize<'de>>(
    bt: &BTreeMap<String, Value>,
    key: &str,
    default: T,
) -> T {
    match bt.get(key) {
        Some(v) => match serde_json::from_value(v.clone()) {
            Ok(s) => s,
            Err(_) => default,
        },
        None => default,
    }
}
