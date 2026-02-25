// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use reqwest::{Client, Response};
use serde::{Deserialize, Serialize};
use serde_json::json;
use unic_langid::LanguageIdentifier;

use super::lang_id_to_string;

// KDE GitLab GraphiQL: https://invent.kde.org/-/graphql-explorer

pub async fn graphql_kde(
    lang_id: LanguageIdentifier,
    client: Client,
) -> Result<Vec<String>, anyhow::Error> {
    let query = format!(
        "
        query {{
            groups {{
                edges {{
                    node {{
                        ...Group
                    }}
                }}
            }}
        }}

        fragment Group on Group {{
            name
            projects {{
                edges {{
                    node {{
                        webUrl
                        repository {{
                            po:   tree(path: \"po/{0}\") {{ ...Tree }}
                            poqm: tree(path: \"poqm/{0}\") {{ ...Tree }}
                        }}
                    }}
                }}
            }}
        }}

        fragment Tree on Tree {{
            blobs {{
                edges {{
                    node {{
                        path
                    }}
                }}
            }}
        }}
        ",
        lang_id_to_string(&lang_id, "_", true, "@", false),
    );

    let response: Response = client
        .post("https://invent.kde.org/api/graphql")
        .json(&json!({"query": query, "variables": ()}))
        .send()
        .await?;
    let response: GraphQl = response.json().await?;

    let mut urls = Vec::new();

    for group in response.data.groups.edges {
        let _group_name = group.node.name;
        for project in group.node.projects.edges {
            let web_url = project.node.web_url;
            if let Some(repository) = project.node.repository {
                if let Some(tree) = repository.po {
                    for blob in tree.blobs.edges {
                        let path = blob.node.path;
                        urls.push(format!("{web_url}/-/raw/HEAD/{path}"));
                    }
                }
                if let Some(tree) = repository.poqm {
                    for blob in tree.blobs.edges {
                        let path = blob.node.path;
                        urls.push(format!("{web_url}/-/raw/HEAD/{path}"));
                    }
                }
            }
        }
    }

    Ok(urls)
}

#[derive(Debug, Deserialize, Serialize)]
struct GraphQl {
    data: Data,
}

#[derive(Debug, Deserialize, Serialize)]
struct Data {
    groups: Groups,
}

#[derive(Debug, Deserialize, Serialize)]
struct Groups {
    edges: Vec<GroupsEdge>,
}

#[derive(Debug, Deserialize, Serialize)]
struct GroupsEdge {
    node: GroupsEdgeNode,
}

#[derive(Debug, Deserialize, Serialize)]
struct GroupsEdgeNode {
    name: String,
    projects: Projects,
}

#[derive(Debug, Deserialize, Serialize)]
struct Projects {
    edges: Vec<ProjectsEdge>,
}

#[derive(Debug, Deserialize, Serialize)]
struct ProjectsEdge {
    node: ProjectsEdgeNode,
}

#[derive(Debug, Deserialize, Serialize)]
struct ProjectsEdgeNode {
    #[serde(rename = "webUrl")]
    web_url: String,
    repository: Option<Repository>,
}

#[derive(Debug, Deserialize, Serialize)]
struct Repository {
    po: Option<Tree>,
    poqm: Option<Tree>,
}

#[derive(Debug, Deserialize, Serialize)]
struct Tree {
    blobs: Blobs,
}

#[derive(Debug, Deserialize, Serialize)]
struct Blobs {
    edges: Vec<BlobsEdge>,
}

#[derive(Debug, Deserialize, Serialize)]
struct BlobsEdge {
    node: BlobsEdgeNode,
}

#[derive(Debug, Deserialize, Serialize)]
struct BlobsEdgeNode {
    path: String,
}
