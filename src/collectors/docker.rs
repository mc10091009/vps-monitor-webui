use std::collections::HashMap;

use bollard::container::ListContainersOptions;
use bollard::Docker;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct DockerContainer {
    pub id: String,
    pub names: Vec<String>,
    pub image: String,
    pub state: String,
    pub status: String,
    pub created: i64,
    pub ports: Vec<String>,
}

pub async fn list(docker: &Docker) -> anyhow::Result<Vec<DockerContainer>> {
    let opts = ListContainersOptions::<String> {
        all: true,
        ..Default::default()
    };
    let cs = docker.list_containers(Some(opts)).await?;
    let out = cs
        .into_iter()
        .map(|c| DockerContainer {
            id: c.id.unwrap_or_default(),
            names: c
                .names
                .unwrap_or_default()
                .into_iter()
                .map(|n| n.trim_start_matches('/').to_string())
                .collect(),
            image: c.image.unwrap_or_default(),
            state: c.state.unwrap_or_default(),
            status: c.status.unwrap_or_default(),
            created: c.created.unwrap_or(0),
            ports: c
                .ports
                .unwrap_or_default()
                .into_iter()
                .filter_map(|p| {
                    let pub_p = p.public_port?;
                    let priv_p = p.private_port;
                    let typ = p.typ.map(|t| format!("{:?}", t).to_lowercase()).unwrap_or_default();
                    Some(format!("{pub_p}->{priv_p}/{typ}"))
                })
                .collect(),
        })
        .collect();
    Ok(out)
}

/// Validate container ID/name is safe (hex or known docker name chars).
pub fn validate_id(id: &str) -> bool {
    if id.is_empty() || id.len() > 256 {
        return false;
    }
    id.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
}

pub async fn inspect_short(docker: &Docker, id: &str) -> anyhow::Result<HashMap<String, String>> {
    let info = docker.inspect_container(id, None).await?;
    let mut m = HashMap::new();
    if let Some(name) = info.name {
        m.insert("name".into(), name.trim_start_matches('/').to_string());
    }
    if let Some(state) = info.state {
        if let Some(s) = state.status {
            m.insert("status".into(), format!("{:?}", s));
        }
    }
    Ok(m)
}
