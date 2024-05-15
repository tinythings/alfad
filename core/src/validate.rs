use std::collections::HashMap;

use tracing::{error, warn};

use crate::task::TaskConfig;

pub fn validate(configs: Vec<TaskConfig>) -> Vec<TaskConfig> {
    let map: HashMap<_, _> = configs
        .iter()
        .map(|e| {
            let mut deps = e.after.to_vec();
            deps.extend(e.with.clone());
            (e.name.clone(), deps)
        })
        .collect();
    configs.iter().for_each(|task| {
        has_loop(task.name.clone(), &map, &[]);
    });
    configs
    // configs.into_iter().filter(|task| !has_loop(task.name.clone(), &map, &vec![])).collect()
}

fn has_loop(name: String, map: &HashMap<String, Vec<String>>, visited: &[String]) -> bool {
    if visited.contains(&name) {
        if visited.len() == 1 {
            warn!("{name} is waiting for itself and will never run")
        } else {
            warn!(
                "{name} is waiting for a loop and will never run ({} -> {name})",
                visited.join(" -> ")
            );
        }
        return true;
    }
    let mut visited = visited.to_owned();
    visited.push(name.clone());
    if let Some(list) = map.get(&name) {
        list.iter().any(|b| has_loop(b.clone(), map, &visited))
    } else {
        error!("No task named {name}");
        false
    }
}
