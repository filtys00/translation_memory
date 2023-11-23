use std::sync::Arc;

use reqwest::{Client, Response};
use serde::{Deserialize, Serialize};
use serde_json::json;
use unic_langid::LanguageIdentifier;

pub async fn graphql_gnome(
    lang_id: LanguageIdentifier,
    client: Arc<Client>,
) -> Result<Vec<String>, anyhow::Error> {
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
        lang_id.language
    );

    let response: Response = client
        .post("https://gitlab.gnome.org/api/graphql")
        .json(&json!({"query": query, "variables": ()}))
        .send()
        .await?;
    let response: GraphQl = response.json().await?;

    let mut urls = Vec::new();

    for project in response.data.gnome.projects.edges {
        let web_url = project.node.web_url;
        for blob in project.node.repository.blobs.edges {
            let path = blob.node.path;
            urls.push(format!("{web_url}/-/raw/master/{path}"));
        }
    }
    for project in response.data.world.projects.edges {
        let web_url = project.node.web_url;
        for blob in project.node.repository.blobs.edges {
            let path = blob.node.path;
            urls.push(format!("{web_url}/-/raw/master/{path}"));
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
