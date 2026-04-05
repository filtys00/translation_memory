// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use reqwest::Url;
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::{DbProvider, DbSourceUrls, Downloader, LangId};

// XFCE GitLab GraphiQL: https://gitlab.xfce.org/-/graphql-explorer

pub fn get_xfce_sources(lang_ids: &[LangId], provider: &DbProvider, downloader: &Downloader) -> anyhow::Result<()> {
    let query = "
        query {
            groups {
                edges {
                    node {
                        ...Group
                    }
                }
            }
        }

        fragment Group on Group {
            name
            projects {
                edges {
                    node {
                        webUrl
                        repository {
                            tree(path: \"po\") { ...Tree }
                        }
                    }
                }
            }
        }

        fragment Tree on Tree {
            blobs {
                edges {
                    node {
                        path
                    }
                }
            }
        }
    ";

    let response: GraphQl = downloader.post_json(
        "https://gitlab.xfce.org/api/graphql",
        json!({"query": query, "variables": ()}),
    )?;

    
    for lang_id in lang_ids {
        let lang_path = format!("po/{}.po", lang_id.format("_", true, "@", false));

        let mut urls = Vec::new();

        for group in &response.data.groups.edges {
            let _group_name = &group.node.name;
            for project in &group.node.projects.edges {
                let web_url = &project.node.web_url;
                if let Some(repository) = &project.node.repository && let Some(tree) = &repository.tree {
                    for blob in &tree.blobs.edges {
                        let path = &blob.node.path;
                        if *path != lang_path { continue; }
                        urls.push(DbSourceUrls {
                            originals: None,
                            translations: Url::parse(&format!("{web_url}/-/raw/HEAD/{path}"))?,
                        });
                    }
                }
            }
        }

        provider.set_sources(lang_id, &urls)?;
    }

    Ok(())
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
    tree: Option<Tree>,
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
