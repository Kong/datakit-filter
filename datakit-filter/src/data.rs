use std::collections::BTreeMap;

use crate::dependency_graph::DependencyGraph;

#[derive(Debug, PartialEq)]
pub enum Payload {
    Raw(Vec<u8>),
    Json(serde_json::Value),
}

impl Payload {
    pub fn from_bytes(bytes: Vec<u8>, content_type: Option<String>) -> Option<Payload> {
        match content_type {
            Some(ct) => {
                if ct == "application/json" {
                    match serde_json::from_slice(&bytes) {
                        Ok::<serde_json::Value, _>(v) => Some(Payload::Json(v)),
                        Err::<_, serde_json::Error>(e) => {
                            log::error!("error decoding json: {}", e);

                            None
                        }
                    }
                } else {
                    Some(Payload::Raw(bytes))
                }
            }
            _ => None,
        }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        match &self {
            Payload::Json(value) => {
                match serde_json::to_string(value) {
                    Ok(s) => s.into_bytes(),
                    Err::<_, serde_json::Error>(e) => {
                        log::error!("error decoding json: {}", e);
                        // FIXME should we return Result instead?
                        vec![]
                    }
                }
            }
            Payload::Raw(s) => s.clone(), // it would be nice to be able to avoid this copy
        }
    }

    pub fn to_headers_vec(&self) -> Vec<(&str, &str)> {
        // TODO
        log::debug!("NYI: converting payload into headers vector");
        vec![]
    }
}

#[derive(PartialEq, Debug)]
pub enum State {
    // Ready(),
    Waiting(u32),
    Done(Option<Payload>),
}

#[derive(Default)]
pub struct Data {
    graph: DependencyGraph,
    states: BTreeMap<String, State>,
}

impl Data {
    pub fn new(graph: DependencyGraph) -> Data {
        Data {
            graph,
            states: Default::default(),
        }
    }

    pub fn set(&mut self, name: &str, state: State) {
        self.states.insert(name.to_string(), state);
    }

    pub fn get_inputs_for(&self, name: &str, waiting: Option<u32>) -> Option<Vec<&Payload>> {
        // If node is Done, avoid producing inputs
        // and re-triggering its execution.
        if let Some(state) = self.states.get(name) {
            match state {
                State::Done(_) => {
                    return None;
                }
                State::Waiting(w) => match &waiting {
                    Some(id) => {
                        if w != id {
                            return None;
                        }
                    }
                    None => return None,
                },
                // State::Ready() => {}
            }
        }

        // Check that all inputs have payloads available
        for input in self.graph.each_input(name) {
            let val = self.states.get(input);
            match val {
                Some(State::Done(_)) => {}
                _ => {
                    return None;
                }
            };
        }

        // If so, allocate the vector with the result.
        let mut vec: Vec<&Payload> = Vec::new();
        for input in self.graph.each_input(name) {
            if let Some(State::Done(Some(p))) = self.states.get(input) {
                vec.push(p);
            }
        }

        Some(vec)
    }
}
