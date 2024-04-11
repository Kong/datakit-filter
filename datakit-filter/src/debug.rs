use crate::config::Config;
use crate::data::{Payload, State};
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;

pub enum RunMode {
    Run,
    Resume,
}

pub enum DataMode {
    Done,
    Waiting,
    Fail,
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
    value: Option<Value>,
}

enum Operation {
    Run(RunOperation),
    Set(SetOperation),
}

pub struct Debug {
    trace: bool,
    operations: Vec<Operation>,
    node_types: HashMap<String, String>,
    orig_response_body_content_type: Option<String>,
}

impl State {
    fn to_data_mode(&self) -> DataMode {
        match self {
            State::Done(_) => DataMode::Done,
            State::Waiting(_) => DataMode::Waiting,
            State::Fail(_) => DataMode::Fail,
        }
    }
}

fn payload_to_op_info(p: &Option<Payload>, default_type: &str) -> (String, Option<Value>) {
    if let Some(payload) = p {
        let dt = payload.content_type().unwrap_or(default_type).to_string();

        match payload.to_json() {
            Ok(v) => (dt, Some(v)),
            Err(e) => ("fail".to_string(), Some(serde_json::json!(e))),
        }
    } else {
        ("none".to_string(), None)
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
            orig_response_body_content_type: None,
        }
    }

    pub fn set_data(&mut self, name: &str, state: &State) {
        if self.trace {
            let (data_type, value) = match state {
                State::Done(p) => payload_to_op_info(p, "raw"),
                State::Waiting(_) => ("waiting".to_string(), None),
                State::Fail(p) => payload_to_op_info(p, "fail"),
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

    pub fn save_response_body_content_type(&mut self, ct: Option<String>) {
        self.orig_response_body_content_type = ct;
    }

    pub fn response_body_content_type(&self) -> &Option<String> {
        &self.orig_response_body_content_type
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
            #[serde(skip_serializing_if = "Option::is_none")]
            r#type: Option<&'a str>,
            #[serde(skip_serializing_if = "Option::is_none")]
            value: Option<&'a Value>,
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
                    DataMode::Fail => TraceAction {
                        action: "fail",
                        name: &set.node_name,
                        r#type: None,
                        value: set.value.as_ref(),
                    },
                },
            });
        }

        serde_json::json!(actions).to_string()
    }
}
