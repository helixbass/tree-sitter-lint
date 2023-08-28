pub type Environment = serde_json::Map<String, serde_json::Value>;

pub fn deep_merged(a: &Environment, b: &Environment) -> Environment {
    let mut merged: Environment = a.clone();

    deep_merge(&mut merged, b);

    merged
}

fn deep_merge(a: &mut Environment, b: &Environment) {
    for (key, value) in b {
        match a.get_mut(key) {
            Some(existing_value) => {
                match (existing_value, value) {
                    (serde_json::Value::Array(existing_value), serde_json::Value::Array(value)) => {
                        existing_value.extend(value.into_iter().cloned());
                    }
                    (serde_json::Value::Object(existing_value), serde_json::Value::Object(value)) => {
                        deep_merge(existing_value, value);
                    }
                    (existing_value, value) => {
                        *existing_value = value.clone();
                    }
                }
            }
            None => {
                a.insert(key.clone(), value.clone());
            }
        }
    }
}
