use crate::config::Config;
use crate::data::{Payload, State};
use serde::Serialize;
use std::collections::HashMap;

pub enum RunMode {
    Run,
    Resume,
}

pub enum DataMode {
    Done,
    Waiting,
}

struct RunOperation {
    node_name: String,
    node_type: String,
    action: RunMode,
}

struct SetOperation {
    node_name: String,
    data_type: String,
    status: DataMode,
    value: Option<serde_json::Value>,
}

enum Operation {
    Run(RunOperation),
    Set(SetOperation),
}

pub struct Debug {
    trace: bool,
    operations: Vec<Operation>,
    node_types: HashMap<String, String>,
}

impl State {
    fn to_data_mode(&self) -> DataMode {
        match self {
            State::Done(_) => DataMode::Done,
            State::Waiting(_) => DataMode::Waiting,
        }
    }
}

impl Debug {
    pub fn new(config: &Config) -> Debug {
        let mut node_types = HashMap::new();
        for (name, node_type) in config.node_types() {
            node_types.insert(name.to_string(), node_type.to_string());
        }

        Debug {
            node_types,
            trace: false,
            operations: vec![],
        }
    }

    pub fn set_data(&mut self, name: &str, state: &State) {
        if self.trace {
            let (data_type, value) = match state {
                State::Done(d) => {
                    if let Some(p) = d {
                        let dt = p.content_type().unwrap_or("raw").to_string();
                        let v = p.to_json().ok();

                        (dt, v)
                    } else {
                        ("none".to_string(), None)
                    }
                }
                State::Waiting(_) => ("waiting".to_string(), None),
            };

            self.operations.push(Operation::Set(SetOperation {
                node_name: name.to_string(),
                data_type,
                status: state.to_data_mode(),
                value,
            }));
        }
    }

    pub fn run(&mut self, name: &str, _args: &[Option<&Payload>], state: &State, action: RunMode) {
        if self.trace {
            let node_type = self.node_types.get(name).expect("node exists");

            self.operations.push(Operation::Run(RunOperation {
                action,
                node_name: name.to_string(),
                node_type: node_type.to_string(),
            }));

            self.set_data(name, state);
        }
    }

    pub fn set_tracing(&mut self, enable: bool) {
        self.trace = enable;
    }

    pub fn is_tracing(&self) -> bool {
        self.trace
    }

    pub fn get_trace(&self) -> String {
        #[derive(Serialize)]
        struct TraceAction<'a> {
            action: &'static str,
            name: &'a str,
            r#type: Option<&'a str>,
            value: Option<&'a serde_json::Value>,
        }

        let mut actions: Vec<TraceAction> = vec![];

        for op in self.operations.iter() {
            actions.push(match op {
                Operation::Run(run) => TraceAction {
                    action: match run.action {
                        RunMode::Run => "run",
                        RunMode::Resume => "resume",
                    },
                    name: &run.node_name,
                    r#type: Some(&run.node_type),
                    value: None,
                },
                Operation::Set(set) => match set.status {
                    DataMode::Done => TraceAction {
                        action: "value",
                        name: &set.node_name,
                        r#type: Some(&set.data_type),
                        value: set.value.as_ref(),
                    },
                    DataMode::Waiting => TraceAction {
                        action: "wait",
                        name: &set.node_name,
                        r#type: None,
                        value: None,
                    },
                },
            });
        }

        serde_json::json!(actions).to_string()
    }
}
