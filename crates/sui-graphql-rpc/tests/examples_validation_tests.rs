// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//#[cfg(feature = "pg_integration")]
mod tests {
    use markdown_gen::markdown::{AsMarkdown, Markdown};
    use rand::rngs::StdRng;
    use rand::SeedableRng;
    use serial_test::serial;
    use simulacrum::Simulacrum;
    use std::fs::File;
    use std::io::{BufWriter, Read, Write};
    use std::path::PathBuf;
    use std::sync::Arc;
    use sui_graphql_rpc::cluster::SimulatorCluster;
    use sui_graphql_rpc::config::ConnectionConfig;

    #[derive(Debug)]
    struct ExampleQuery {
        pub name: String,
        pub contents: String,
        pub path: PathBuf,
    }

    #[derive(Debug)]
    struct ExampleQueryGroup {
        pub name: String,
        pub queries: Vec<ExampleQuery>,
        pub _path: PathBuf,
    }

    const QUERY_EXT: &str = "graphql";

    fn regularize_string(s: &str) -> String {
        // Replace underscore with space and make every word first letter uppercase
        s.replace('_', " ")
            .split_whitespace()
            .map(|word| {
                let mut chars = word.chars();
                match chars.next() {
                    None => String::new(),
                    Some(f) => f.to_uppercase().chain(chars).collect(),
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    #[test]
    fn verify_examples_x() {
        let groups = verify_examples_impl();

        let mut out_file = File::create("docs/examples.md").unwrap();
        let mut output = BufWriter::new(Vec::new());
        let mut md = Markdown::new(&mut output);

        md.write("Sui GraphQL Examples".heading(1)).unwrap();

        // TODO: reduce multiple loops
        // Generate the table of contents
        for (id, group) in groups.iter().enumerate() {
            let group_name = regularize_string(&group.name);
            let group_name_toc = format!("[{}](#{})", group_name, id);
            md.write(group_name_toc.heading(3)).unwrap();

            for (inner, query) in group.queries.iter().enumerate() {
                let inner_id = inner + 0xFFFF * id;
                let inner_name = regularize_string(&query.name);

                let inner_name_toc = format!("&emsp;&emsp;[{}](#{})", inner_name, inner_id);
                md.write(inner_name_toc.heading(4)).unwrap();
            }
        }

        for (id, group) in groups.iter().enumerate() {
            let group_name = regularize_string(&group.name);

            let id_tag = format!("<a id={}></a>", id);
            md.write(id_tag.heading(2)).unwrap();
            md.write(group_name.heading(2)).unwrap();
            for (inner, query) in group.queries.iter().enumerate() {
                let inner_id = inner + 0xFFFF * id;
                let name = regularize_string(&query.name);

                let id_tag = format!("<a id={}></a>", inner_id);
                md.write(id_tag.heading(3)).unwrap();
                md.write(name.heading(3)).unwrap();

                // Extract all lines that start with `#` and use them as headers
                let mut headers = vec![];
                let mut query_start = 0;
                for (idx, line) in query.contents.lines().enumerate() {
                    let line = line.trim();
                    if line.starts_with('#') {
                        headers.push(line.trim_start_matches('#'));
                    } else if line.starts_with('{') {
                        query_start = idx;
                        break;
                    }
                }

                // Remove headers from query
                let query = query
                    .contents
                    .lines()
                    .skip(query_start)
                    .collect::<Vec<_>>()
                    .join("\n");

                let content = format!("<pre>{}</pre>", query);
                for header in headers {
                    md.write(header.heading(4)).unwrap();
                }
                md.write(content.quote()).unwrap();
            }
        }
        let bytes = output.into_inner().unwrap();
        let string = String::from_utf8(bytes).unwrap().replace('\\', "");

        // write string to out_file
        out_file.write_all(string.as_bytes()).unwrap();
    }

    fn verify_examples_impl() -> Vec<ExampleQueryGroup> {
        let mut buf: PathBuf = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        buf.push("examples");

        let mut groups = vec![];
        for entry in std::fs::read_dir(buf).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            let group_name = path.file_stem().unwrap().to_str().unwrap().to_string();

            let mut group = ExampleQueryGroup {
                name: group_name.clone(),
                queries: vec![],
                _path: path.clone(),
            };

            for file in std::fs::read_dir(path).unwrap() {
                assert!(file.is_ok());
                let file = file.unwrap();
                assert!(file.path().extension().is_some());
                let ext = file
                    .path()
                    .extension()
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .to_string();
                assert_eq!(ext, QUERY_EXT);

                let file_path = file.path();
                let query_name = file_path.file_stem().unwrap().to_str().unwrap().to_string();

                let mut contents = String::new();
                let mut fp = std::fs::File::open(file_path.clone()).unwrap();
                fp.read_to_string(&mut contents).unwrap();
                group.queries.push(ExampleQuery {
                    name: query_name,
                    contents,
                    path: file_path,
                });
            }

            groups.push(group);
        }
        groups
    }

    fn bad_examples() -> ExampleQueryGroup {
        ExampleQueryGroup {
            name: "bad_examples".to_string(),
            queries: vec![
                ExampleQuery {
                    name: "multiple_queries".to_string(),
                    contents: "{ chainIdentifier } { chainIdentifier }".to_string(),
                    path: PathBuf::from("multiple_queries.graphql"),
                },
                ExampleQuery {
                    name: "malformed".to_string(),
                    contents: "query { }}".to_string(),
                    path: PathBuf::from("malformed.graphql"),
                },
                ExampleQuery {
                    name: "invalid".to_string(),
                    contents: "djewfbfo".to_string(),
                    path: PathBuf::from("invalid.graphql"),
                },
                ExampleQuery {
                    name: "empty".to_string(),
                    contents: "     ".to_string(),
                    path: PathBuf::from("empty.graphql"),
                },
            ],
            _path: PathBuf::from("bad_examples"),
        }
    }

    async fn validate_example_query_group(
        cluster: &SimulatorCluster,
        group: &ExampleQueryGroup,
    ) -> Vec<String> {
        let mut errors = vec![];
        for query in &group.queries {
            let resp = cluster
                .graphql_client
                .execute(query.contents.clone(), vec![])
                .await
                .unwrap();
            if let Some(err) = resp.get("errors") {
                errors.push(format!(
                    "Query failed: {}: {} at: {}\nError: {}",
                    group.name,
                    query.name,
                    query.path.display(),
                    err
                ));
            }
        }
        errors
    }

    #[tokio::test]
    #[serial]
    async fn test_single_all_examples_structure_valid() {
        let rng = StdRng::from_seed([12; 32]);
        let mut sim = Simulacrum::new_with_rng(rng);

        sim.create_checkpoint();

        let connection_config = ConnectionConfig::ci_integration_test_cfg();

        let cluster =
            sui_graphql_rpc::cluster::serve_simulator(connection_config, 3000, Arc::new(sim)).await;

        let groups = verify_examples_impl();

        let mut errors = vec![];
        for group in groups {
            errors.extend(validate_example_query_group(&cluster, &group).await);
        }

        assert!(errors.is_empty(), "\n{}", errors.join("\n\n"));
    }

    #[tokio::test]
    #[serial]
    async fn test_bad_examples_fail() {
        let rng = StdRng::from_seed([12; 32]);
        let mut sim = Simulacrum::new_with_rng(rng);

        sim.create_checkpoint();

        let connection_config = ConnectionConfig::ci_integration_test_cfg();

        let cluster =
            sui_graphql_rpc::cluster::serve_simulator(connection_config, 3000, Arc::new(sim)).await;

        let bad_examples = bad_examples();
        let errors = validate_example_query_group(&cluster, &bad_examples).await;

        assert_eq!(
            errors.len(),
            bad_examples.queries.len(),
            "all examples should fail"
        );
    }
}
