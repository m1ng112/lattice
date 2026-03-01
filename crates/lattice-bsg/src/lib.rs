pub mod bsg {
    include!(concat!(env!("OUT_DIR"), "/lattice.bsg.rs"));
}

pub mod convert;

use prost::Message;
use std::path::Path;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum BsgError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("protobuf decode error: {0}")]
    Decode(#[from] prost::DecodeError),
}

/// Write a BSG graph to a binary protobuf file.
pub fn write_bsg(graph: &bsg::Graph, path: &Path) -> Result<(), BsgError> {
    let bytes = graph.encode_to_vec();
    std::fs::write(path, bytes)?;
    Ok(())
}

/// Read a BSG graph from a binary protobuf file.
pub fn read_bsg(path: &Path) -> Result<bsg::Graph, BsgError> {
    let bytes = std::fs::read(path)?;
    let graph = bsg::Graph::decode(bytes.as_slice())?;
    Ok(graph)
}

/// Pretty-print a BSG graph for debugging.
pub fn dump_bsg(graph: &bsg::Graph) -> String {
    let mut out = String::new();
    out.push_str(&format!("Graph: {} (v{})\n", graph.id, graph.version));

    out.push_str(&format!("  Nodes: {}\n", graph.nodes.len()));
    for node in &graph.nodes {
        let kind = bsg::NodeKind::try_from(node.kind).unwrap_or(bsg::NodeKind::Compute);
        out.push_str(&format!("    [{}] kind={kind:?}\n", node.id));
        for inp in &node.inputs {
            out.push_str(&format!(
                "      input {}: {}\n",
                inp.name,
                type_to_string(inp.r#type.as_ref())
            ));
        }
        for outp in &node.outputs {
            out.push_str(&format!(
                "      output {}: {}\n",
                outp.name,
                type_to_string(outp.r#type.as_ref())
            ));
        }
        if let Some(sem) = &node.semantic {
            if !sem.natural_language.is_empty() {
                out.push_str(&format!("      semantic: \"{}\"\n", sem.natural_language));
            }
        }
        if let Some(props) = &node.properties {
            if props.idempotent {
                out.push_str("      idempotent: true\n");
            }
            if props.deterministic {
                out.push_str("      deterministic: true\n");
            }
        }
    }

    out.push_str(&format!("  Edges: {}\n", graph.edges.len()));
    for edge in &graph.edges {
        out.push_str(&format!(
            "    {}.{} -> {}.{}\n",
            edge.source_node, edge.source_port, edge.target_node, edge.target_port
        ));
        if let Some(props) = &edge.properties {
            if props.r#async {
                out.push_str("      async: true\n");
            }
            if props.buffered {
                out.push_str("      buffered: true\n");
            }
        }
    }

    if !graph.proofs.is_empty() {
        out.push_str(&format!("  Proofs: {}\n", graph.proofs.len()));
    }

    out
}

fn type_to_string(ty: Option<&bsg::Type>) -> String {
    match ty.and_then(|t| t.kind.as_ref()) {
        Some(bsg::r#type::Kind::Primitive(p)) => p.name.clone(),
        Some(bsg::r#type::Kind::Stream(s)) => {
            format!("Stream<{}>", type_to_string(s.element.as_deref()))
        }
        Some(bsg::r#type::Kind::Distribution(d)) => {
            format!("Distribution<{}>", type_to_string(d.inner.as_deref()))
        }
        Some(bsg::r#type::Kind::Function(f)) => {
            let params: Vec<_> = f.params.iter().map(|p| type_to_string(Some(p))).collect();
            format!(
                "({}) -> {}",
                params.join(", "),
                type_to_string(f.return_type.as_deref())
            )
        }
        Some(bsg::r#type::Kind::Product(p)) => {
            let fields: Vec<_> = p
                .fields
                .iter()
                .map(|f| format!("{}: {}", f.name, type_to_string(f.r#type.as_ref())))
                .collect();
            format!("{{ {} }}", fields.join(", "))
        }
        Some(bsg::r#type::Kind::Sum(s)) => {
            let variants: Vec<_> = s
                .variants
                .iter()
                .map(|v| match &v.payload {
                    Some(payload) => format!("{}({})", v.name, type_to_string(Some(payload))),
                    None => v.name.clone(),
                })
                .collect();
            variants.join(" | ")
        }
        Some(bsg::r#type::Kind::Refinement(r)) => {
            format!(
                "{{ x in {} | {} }}",
                type_to_string(r.base.as_deref()),
                r.predicate
            )
        }
        Some(bsg::r#type::Kind::Dependent(d)) => {
            if d.params.is_empty() {
                d.name.clone()
            } else {
                let params: Vec<_> = d
                    .params
                    .iter()
                    .map(|p| format!("{}: {}", p.name, type_to_string(p.r#type.as_ref())))
                    .collect();
                format!("{}({})", d.name, params.join(", "))
            }
        }
        None => "?".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lattice_parser::parser;

    #[test]
    fn test_ast_to_bsg_simple_graph() {
        let source = r#"
graph HelloWorld {
  version: "0.1.0"

  node Greeter {
    input: String
    output: String

    semantic: {
      description: "Produces a greeting"
    }
  }

  node Printer {
    input: String

    properties: {
      idempotent: true
    }
  }

  edge Greeter -> Printer {
    buffered: true
  }
}
"#;
        let program = parser::parse(source).unwrap();
        let graph = convert::ast_to_bsg(&program);

        assert_eq!(graph.id, "HelloWorld");
        assert_eq!(graph.version, "0.1.0");
        assert_eq!(graph.nodes.len(), 2);
        assert_eq!(graph.edges.len(), 1);

        let greeter = &graph.nodes[0];
        assert_eq!(greeter.id, "Greeter");
        assert_eq!(greeter.inputs.len(), 1);
        assert_eq!(greeter.outputs.len(), 1);
        assert!(greeter.semantic.is_some());
        assert_eq!(
            greeter.semantic.as_ref().unwrap().natural_language,
            "Produces a greeting"
        );

        let printer = &graph.nodes[1];
        assert_eq!(printer.id, "Printer");
        assert!(printer.properties.as_ref().unwrap().idempotent);

        let edge = &graph.edges[0];
        assert_eq!(edge.source_node, "Greeter");
        assert_eq!(edge.target_node, "Printer");
        assert!(edge.properties.as_ref().unwrap().buffered);
    }

    #[test]
    fn test_write_read_roundtrip() {
        let source = r#"
graph Test {
  version: "1.0"

  node A {
    input: Int
    output: String
  }

  node B {
    input: String
  }

  edge A -> B
}
"#;
        let program = parser::parse(source).unwrap();
        let graph = convert::ast_to_bsg(&program);

        let dir = std::env::temp_dir();
        let path = dir.join("lattice_bsg_test_roundtrip.bsg");

        write_bsg(&graph, &path).unwrap();
        let loaded = read_bsg(&path).unwrap();

        assert_eq!(graph, loaded);

        // Clean up
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_bsg_to_ast_roundtrip() {
        let source = r#"
graph RoundTrip {
  version: "0.2.0"

  node Source {
    input: Int
    output: String

    semantic: {
      description: "Converts int to string"
    }
  }

  node Sink {
    input: String

    properties: {
      idempotent: true
    }
  }

  edge Source -> Sink {
    buffered: true
  }
}
"#;
        let program = parser::parse(source).unwrap();
        let bsg_graph = convert::ast_to_bsg(&program);
        let ast_roundtrip = convert::bsg_to_ast(&bsg_graph);

        // The roundtripped AST should produce the same BSG
        let bsg_again = convert::ast_to_bsg(&ast_roundtrip);
        assert_eq!(bsg_graph, bsg_again);
    }

    #[test]
    fn test_bsg_dump() {
        let graph = bsg::Graph {
            id: "TestGraph".into(),
            version: "1.0".into(),
            nodes: vec![bsg::Node {
                id: "MyNode".into(),
                kind: bsg::NodeKind::Compute as i32,
                inputs: vec![bsg::TypedPort {
                    name: "default".into(),
                    r#type: Some(bsg::Type {
                        kind: Some(bsg::r#type::Kind::Primitive(bsg::PrimitiveType {
                            name: "Int".into(),
                        })),
                    }),
                }],
                ..Default::default()
            }],
            edges: vec![bsg::Edge {
                source_node: "A".into(),
                source_port: "out".into(),
                target_node: "B".into(),
                target_port: "in".into(),
                ..Default::default()
            }],
            ..Default::default()
        };

        let dump = dump_bsg(&graph);
        assert!(dump.contains("TestGraph"));
        assert!(dump.contains("MyNode"));
        assert!(dump.contains("Int"));
        assert!(dump.contains("A.out -> B.in"));
    }

    #[test]
    fn test_type_conversion_roundtrip() {
        // Named type
        let ast_ty = lattice_parser::ast::TypeExpr::Named("Int".into());
        let bsg_ty = convert::convert_type_expr_pub(&ast_ty);
        let back = convert::bsg_type_to_ast_pub(&bsg_ty);
        assert!(matches!(back, lattice_parser::ast::TypeExpr::Named(n) if n == "Int"));

        // Stream type
        let ast_ty = lattice_parser::ast::TypeExpr::Stream(Box::new(
            lattice_parser::ast::Spanned::dummy(lattice_parser::ast::TypeExpr::Named(
                "Event".into(),
            )),
        ));
        let bsg_ty = convert::convert_type_expr_pub(&ast_ty);
        let back = convert::bsg_type_to_ast_pub(&bsg_ty);
        match back {
            lattice_parser::ast::TypeExpr::Stream(inner) => {
                assert!(matches!(&inner.node, lattice_parser::ast::TypeExpr::Named(n) if n == "Event"));
            }
            _ => panic!("expected Stream type"),
        }
    }

    #[test]
    fn test_empty_graph() {
        let graph = bsg::Graph::default();
        let ast = convert::bsg_to_ast(&graph);
        assert_eq!(ast.len(), 1);
        if let lattice_parser::ast::Item::Graph(g) = &ast[0].node {
            assert!(g.members.is_empty());
            assert!(g.version.is_none());
        } else {
            panic!("expected graph");
        }
    }
}
