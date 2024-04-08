use crate::nodes;
use crate::nodes::{NodeConfig, NodeMap};
use crate::DependencyGraph;
use lazy_static::lazy_static;
use serde::de::{Error, MapAccess, Visitor};
use serde::{Deserialize, Deserializer};
use serde_json::Value;
use serde_json_wasm::de;
use std::collections::BTreeMap;
use std::collections::HashSet;
use std::fmt;

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

pub struct UserNodeConfig {
    node_type: String,
    name: String,
    bt: BTreeMap<String, serde_json::Value>,
    inputs: Vec<String>,
    outputs: Vec<String>,
}

impl<'a> Deserialize<'a> for UserNodeConfig {
    fn deserialize<D>(de: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'a>,
    {
        struct UserNodeConfigVisitor;

        impl<'de> Visitor<'de> for UserNodeConfigVisitor {
            type Value = UserNodeConfig;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a DataKit node config")
            }

            fn visit_map<M>(self, mut map: M) -> Result<Self::Value, M::Error>
            where
                M: MapAccess<'de>,
            {
                let mut bt = BTreeMap::new();
                let mut typ: Option<String> = None;
                let mut name: Option<String> = None;
                let mut inputs = Vec::new();
                let mut outputs = Vec::new();
                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "type" => {
                            if let Ok(serde_json::Value::String(value)) = map.next_value() {
                                typ = Some(value);
                            }
                        }
                        "name" => {
                            if let Ok(serde_json::Value::String(value)) = map.next_value() {
                                name = Some(value);
                            }
                        }
                        "input" => {
                            if let Ok(serde_json::Value::String(value)) = map.next_value() {
                                inputs.push(value);
                            }
                        }
                        "inputs" => {
                            if let Ok(values) = map.next_value() {
                                if let Ok(v) = serde_json::from_value::<Vec<String>>(values) {
                                    inputs = v;
                                }
                            }
                        }
                        "output" => {
                            if let Ok(serde_json::Value::String(value)) = map.next_value() {
                                outputs.push(value);
                            }
                        }
                        "outputs" => {
                            if let Ok(values) = map.next_value() {
                                if let Ok(v) = serde_json::from_value::<Vec<String>>(values) {
                                    outputs = v;
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

                if let Some(node_type) = typ {
                    let name = name.unwrap_or_else(|| format!("{:p}", &bt));
                    Ok(UserNodeConfig {
                        node_type,
                        name,
                        bt,
                        inputs,
                        outputs,
                    })
                } else {
                    Err(Error::missing_field("type"))
                }
            }
        }

        de.deserialize_map(UserNodeConfigVisitor)
    }
}

#[derive(Deserialize, Default)]
pub struct UserConfig {
    nodes: Vec<UserNodeConfig>,
}

struct NodeInfo {
    name: String,
    node_type: String,
    node_config: Box<dyn NodeConfig>,
}

pub struct Config {
    node_list: Vec<NodeInfo>,
    node_names: Vec<String>,
    graph: DependencyGraph,
}

impl Config {
    pub fn new(config_bytes: Vec<u8>) -> Result<Config, String> {
        match de::from_slice::<UserConfig>(&config_bytes) {
            Ok(user_config) => {
                let mut node_list = Vec::new();
                let mut node_names = Vec::new();
                let mut graph: DependencyGraph = Default::default();

                for unc in &user_config.nodes {
                    let name: &str = &unc.name;

                    if RESERVED_NODE_NAMES.contains(name) {
                        return Err(format!("cannot use reserved node name '{}'", name));
                    }

                    node_names.push(name.to_string());
                    for input in &unc.inputs {
                        graph.add(input, name);
                    }
                    for output in &unc.outputs {
                        graph.add(name, output);
                    }
                }

                for unc in &user_config.nodes {
                    let inputs = graph.get_input_names(&unc.name);
                    match nodes::new_config(&unc.node_type, &unc.name, inputs, &unc.bt) {
                        Ok(nc) => node_list.push(NodeInfo {
                            name: unc.name.to_string(),
                            node_type: unc.node_type.to_string(),
                            node_config: nc,
                        }),
                        Err(err) => {
                            return Err(err);
                        }
                    };
                }

                Ok(Config {
                    node_list,
                    node_names,
                    graph,
                })
            }
            Err(err) => Err(format!(
                "failed parsing configuration: {}: {}",
                String::from_utf8(config_bytes).unwrap(),
                err
            )),
        }
    }

    pub fn get_node_names(&self) -> &Vec<String> {
        &self.node_names
    }

    pub fn get_graph(&self) -> &DependencyGraph {
        &self.graph
    }

    pub fn build_nodes(&self) -> NodeMap {
        let mut nodes = NodeMap::new();

        for info in &self.node_list {
            let name = &info.name;
            let node_config = &info.node_config;

            match nodes::new_node(&info.node_type, node_config) {
                Ok(node) => {
                    nodes.insert(name.to_string(), node);
                }
                Err(err) => {
                    log::error!("{}", err);
                }
            }
        }

        nodes
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
