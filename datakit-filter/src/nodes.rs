use core::slice::Iter;
use proxy_wasm::{traits::*};
use serde::de::{Error, MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt;
use std::sync::{Mutex, OnceLock};

use crate::data::{Payload, State, State::*};

pub mod call;
pub mod template;

pub trait Node {
    fn run(&mut self, _ctx: &dyn HttpContext, _inputs: Vec<&Payload>) -> State {
        Done(None)
    }

    fn on_http_call_response(&mut self, _ctx: &dyn HttpContext, _inputs: Vec<&Payload>, _body_size: usize) -> State {
        Done(None)
    }

    fn is_waiting_on(&self, _token_id: u32) -> bool {
        false
    }

    fn get_connections(&self) -> &Connections;

    fn get_name(&self) -> &str {
        &self.get_connections().name
    }

    fn clone_dyn(&self) -> Box<dyn Node>;

    fn from_map(bt: BTreeMap<String, Value>, connections: Connections) -> Box<dyn Node>
    where
        Self: Sized;
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

type NodeFromMapFn = fn(BTreeMap<String, Value>, Connections) -> Box<dyn Node>;

type NodeTypeMap = BTreeMap<String, NodeFromMapFn>;

fn node_types() -> &'static Mutex<NodeTypeMap> {
    static NODE_TYPES: OnceLock<Mutex<NodeTypeMap>> = OnceLock::new();
    NODE_TYPES.get_or_init(|| Mutex::new(BTreeMap::new()))
}

pub fn register_node(name: &str, f: NodeFromMapFn) -> bool {
    node_types().lock().unwrap().insert(String::from(name), f);
    true
}

impl<'a> Deserialize<'a> for Box<dyn Node> {
    fn deserialize<D>(de: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'a>,
    {
        struct DynNodeVisitor;

        impl<'de> Visitor<'de> for DynNodeVisitor {
            type Value = Box<dyn Node>;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a DataKit node")
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
                                typ = Some(value.clone());
                            }
                        }
                        "name" => {
                            if let Ok(serde_json::Value::String(value)) = map.next_value() {
                                connections.name = value.clone();
                                has_name = true;
                            }
                        }
                        "input" => {
                            if let Ok(serde_json::Value::String(value)) = map.next_value() {
                                connections.inputs.push(value.clone());
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
                                connections.outputs.push(value.clone());
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
                    if let Some(from_map_fn) = node_types().lock().unwrap().get(&t) {
                        Ok(from_map_fn(bt, connections))
                    } else {
                        Err(Error::unknown_variant(&t, &[]))
                    }
                } else {
                    Err(Error::missing_field("type"))
                }
            }
        }

        de.deserialize_map(DynNodeVisitor)
    }
}

impl Clone for Box<dyn Node> {
    fn clone(&self) -> Self {
        self.clone_dyn()
    }
}
