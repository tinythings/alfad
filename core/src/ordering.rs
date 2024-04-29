use std::collections::HashMap;

use crate::task::TaskConfig;

pub fn construct_groups(configs: &[TaskConfig]) -> Vec<TaskConfig> {
    let mut map = HashMap::new();
    configs.iter().for_each(|config| {
        config
            .group
            .as_ref()
            .map(|group| format!("~{group}"))
            .map(|group| {
                map.entry(group.clone())
                    .or_insert_with(|| TaskConfig::new(group.clone()))
                    .after(&config.name)
            });
    });
    map.into_values().collect()
}

#[cfg(feature = "before")]
pub fn resolve_before(configs: Vec<TaskConfig>) -> Vec<TaskConfig> {
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
    map.into_values().map(|x| x.take()).collect()
}
