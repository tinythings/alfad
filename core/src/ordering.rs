use std::collections::HashMap;

use itertools::Itertools;
use tracing::warn;

use crate::config::{
    yaml::{PayloadYaml, TaskConfigYaml},
    TaskConfig,
};

pub fn construct_markers(configs: &[TaskConfigYaml]) -> Vec<TaskConfigYaml> {
    let mut map = HashMap::new();
    configs.iter().for_each(|config| {
        config
            .group
            .as_ref()
            .map(|group| format!("group::{group}"))
            .map(|name| {
                map.entry(name.clone())
                    .or_insert_with(|| TaskConfigYaml {
                        name,
                        cmd: PayloadYaml::Marker,
                        ..Default::default()
                    })
                    .after(&config.name)
            });
    });
    configs.iter().for_each(|config| {
        for feature in config.provides.iter() {
            let name = format!("feature::{feature}");
            let mut conf = TaskConfigYaml {
                name: name.clone(),
                cmd: PayloadYaml::Marker,
                ..Default::default()
            };
            conf.after(&config.name);
            if let Some(old) = map.insert(name, conf) {
                warn!("Overriding feature::{feature}, already provided by {}", old.name)
            }
        }
    });
    map.into_values().collect()
}

#[cfg(feature = "before")]
pub fn resolve_before(configs: Vec<TaskConfigYaml>) -> Vec<TaskConfigYaml> {
    // TODO: this can probably be done faster with unsafe then with RefCells
    use std::cell::RefCell;

    use tracing::warn;

    let map: HashMap<_, _> = configs
        .into_iter()
        .map(|config| (config.name.clone(), RefCell::new(config)))
        .collect();

    for (n, v) in map.iter() {
        v.borrow_mut()
            .before
            .drain(..)
            .for_each(|name| match map.get(&name) {
                Some(x) => {
                    x.borrow_mut().after(n);
                }
                None => warn!(
                    "{n} tried to run before {name}, which does not exist ({n} will still run)"
                ),
            });
    }
    map.into_values().map(RefCell::into_inner).collect()
}

pub fn sort(configs: Vec<TaskConfig>) -> Vec<TaskConfig> {
    let mut map: HashMap<_, _> = configs
        .into_iter()
        .map(|config| (config.name.clone(), config))
        .collect();

    let mut sorter = topological_sort::TopologicalSort::<String>::new();
    let mut no_deps = Vec::new();
    for t in map.values() {
        // Tasks without any dependencies should start first since they can always run
        if t.after.is_empty() && t.with.is_empty() {
            no_deps.push(t.name.clone());
            continue;
        }

        // Move markers to the back of the list because they must always wait
        if t.payload.is_marker() {
            continue;
        }

        for d in t.after.iter() {
            // Tasks that wait on non-existent others belong in the back of the list
            if map.contains_key(d) {
                sorter.add_dependency(d.clone(), t.name.clone());
            }
        }
        for d in t.with.iter() {
            // Tasks that wait on non-existent others belong in the back of the list
            if map.contains_key(d) {
                sorter.add_dependency(d.clone(), t.name.clone());
            }
        }
    }
    
    let mut res = no_deps
        .into_iter()
        .flat_map(|x| map.remove(&x))
        .collect_vec();
    res.extend(sorter.flat_map(|x| map.remove(&x)));

    // Add all cyclical and orphaned tasks to the end, we may still want to force start them
    res.extend(map.into_values());
    res
}
