// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use serde::{Deserialize, Serialize};
use serde_json::json;

use super::{Downloader, LangId};

// GNOME GitLab GraphiQL: https://gitlab.gnome.org/-/graphql-explorer

pub fn graphql_gnome(lang_id: &LangId, downloader: &Downloader) -> anyhow::Result<Vec<String>> {
    let query = format!(
        "
		query {{
  			gnome: group(fullPath: \"GNOME\") {{ ...Group }}
  			world: group(fullPath: \"World\") {{ ...Group }}
		}}

		fragment Group on Group {{
  			projects {{
    			edges {{
      				node {{
       	 				webUrl
        				repository {{
          					blobs(paths: [\"po/{}.po\"]) {{
            					edges {{
									node {{
										path
									}}
            					}}
          					}}
        				}}
      				}}
    			}}
  			}}
		}}
        ",
        lang_id.format("_", true, "@", false),
    );

    let response: GraphQl = downloader.post_json(
        "https://gitlab.gnome.org/api/graphql",
        json!({"query": query, "variables": ()}),
    )?;

    let mut urls = Vec::new();

    for project in response.data.gnome.projects.edges {
        let web_url = project.node.web_url;
        for blob in project.node.repository.blobs.edges {
            let path = blob.node.path;
            urls.push(format!("{web_url}/-/raw/HEAD/{path}"));
        }
    }
    for project in response.data.world.projects.edges {
        let web_url = project.node.web_url;
        for blob in project.node.repository.blobs.edges {
            let path = blob.node.path;
            urls.push(format!("{web_url}/-/raw/HEAD/{path}"));
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
    gnome: Group,
    world: Group,
}

#[derive(Debug, Deserialize, Serialize)]
struct Group {
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
    repository: Repository,
}

#[derive(Debug, Deserialize, Serialize)]
struct Repository {
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
