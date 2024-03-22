use core::slice::Iter;
use std::collections::BTreeMap;

#[derive(Default, Clone)]
pub struct DependencyGraph {
    dependents: BTreeMap<String, Vec<String>>,
    providers: BTreeMap<String, Vec<String>>,
    empty: Vec<String>,
}

fn add_to(map: &mut BTreeMap<String, Vec<String>>, key: &str, value: &str) {
    match map.get_mut(key) {
        Some(key_items) => {
            let v = value.to_string();
            if !key_items.contains(&v) {
                key_items.push(v);
            }
        }
        None => {
            map.insert(key.to_string(), vec![value.to_string()]);
        }
    };
}

impl DependencyGraph {
    pub fn new() -> DependencyGraph {
        Default::default()
    }

    pub fn add(&mut self, src: &str, dst: &str) {
        add_to(&mut self.dependents, src, dst);
        add_to(&mut self.providers, dst, src);
    }

    pub fn each_input(&self, name: &str) -> Iter<String> {
        if let Some(items) = self.providers.get(name) {
            items.iter()
        } else {
            // FIXME is there a better way to do this?
            self.empty.iter()
        }
    }
}
