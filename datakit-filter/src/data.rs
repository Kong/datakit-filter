use std::collections::BTreeMap;

use crate::dependency_graph::DependencyGraph;

#[derive(Debug, PartialEq)]
pub enum Payload {
    Raw(Vec<u8>),
    Json(serde_json::Value),
}

impl Payload {
    pub fn from_bytes(bytes: Vec<u8>, content_type: Option<&str>) -> Option<Payload> {
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

    pub fn len(&self) -> Option<usize> {
        match &self {
            Payload::Json(_) => None,
            Payload::Raw(s) => Some(s.len()),
        }
    }

    pub fn to_pwm_headers(&self) -> Vec<(&str, &str)> {
        match &self {
            Payload::Json(value) => {
                let mut vec: Vec<(&str, &str)> = vec![];
                if let serde_json::Value::Object(map) = value {
                    for (k, entry) in map {
                        match entry {
                            serde_json::Value::Array(vs) => {
                                for v in vs {
                                    if let serde_json::Value::String(s) = v {
                                        vec.push((k, s));
                                    }
                                }
                            }

                            // accept string values as well
                            serde_json::Value::String(s) => {
                                vec.push((k, s));
                            }

                            _ => {}
                        }
                    }
                }

                vec
            }
            _ => {
                // TODO
                log::debug!("NYI: converting payload into headers vector");
                vec![]
            }
        }
    }
}

pub fn to_pwm_headers(payload: Option<&Payload>) -> Vec<(&str, &str)> {
    payload.map_or_else(Vec::new, |p| p.to_pwm_headers())
}

/// To use this result in proxy-wasm calls as an Option<&[u8]>, use:
/// data::to_pwm_body(p).as_deref()
pub fn to_pwm_body(payload: Option<&Payload>) -> Option<Box<[u8]>> {
    payload.map(|p| p.to_bytes()).map(Vec::into_boxed_slice)
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

    fn can_trigger(&self, name: &str, waiting: Option<u32>) -> bool {
        // If node is Done, avoid producing inputs
        // and re-triggering its execution.
        if let Some(state) = self.states.get(name) {
            match state {
                State::Done(_) => {
                    return false;
                }
                State::Waiting(w) => match &waiting {
                    Some(id) => {
                        if w != id {
                            return false;
                        }
                    }
                    None => return false,
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
                    return false;
                }
            };
        }

        true
    }

    pub fn get_inputs_for(
        &self,
        name: &str,
        waiting: Option<u32>,
    ) -> Option<Vec<Option<&Payload>>> {
        if !self.can_trigger(name, waiting) {
            return None;
        }

        // If so, allocate the vector with the result.
        let mut vec: Vec<Option<&Payload>> = Vec::new();
        for input in self.graph.each_input(name) {
            if let Some(State::Done(p)) = self.states.get(input) {
                vec.push(p.as_ref());
            }
        }

        Some(vec)
    }

    /// If the node is triggerable, that is, it has all its required
    /// inputs available to trigger (i.e. none of its inputs are in a
    /// `Waiting` state), then return the payload of the first input that
    /// is in a `Done state.
    ///
    /// Note that by returning an `Option<&Payload>` this makes no
    /// distinction between the node being not triggerable or the
    /// node being triggerable via a `Done(None)` input.
    ///
    /// This is not an issue because this function is intended for use
    /// with the implicit nodes (`response_body`, etc.) which are
    /// handled as special cases directly by the filter.
    pub fn first_input_for(&self, name: &str, waiting: Option<u32>) -> Option<&Payload> {
        if !self.can_trigger(name, waiting) {
            return None;
        }

        for input in self.graph.each_input(name) {
            if let Some(State::Done(p)) = self.states.get(input) {
                return p.as_ref();
            }
        }

        None
    }
}
