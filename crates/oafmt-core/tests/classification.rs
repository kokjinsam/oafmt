use oafmt_core::{InputFormat, RouteEdge, classify, format};
use oafmt_oas::{MapKind, ObjectKind, SemanticKind, SequenceKind, Version};

fn has_route(source: &str, kind: SemanticKind, expected: &[RouteEdge]) -> bool {
    classify(source, InputFormat::Yaml)
        .unwrap()
        .ranges
        .iter()
        .any(|classified| classified.kind == kind && classified.route == expected)
}

#[test]
fn oas30_reaches_objects_through_maps_and_sequences() {
    let source = r#"openapi: 3.0.4
info: {title: T, version: v}
servers:
  - url: https://example.test
    variables:
      port:
        default: '443'
paths:
  /pets:
    post:
      parameters:
        - name: body
          in: query
          schema:
            properties:
              id:
                type: integer
      callbacks:
        done:
          '{$request.body#/url}':
            post:
              responses:
                '200':
                  description: ok
      responses:
        default:
          description: no
security:
  - oauth: [read]
components:
  schemas:
    Pet:
      allOf:
        - properties:
            name:
              type: string
"#;
    let result = classify(source, InputFormat::Yaml).unwrap();
    assert_eq!(result.version, Version::Oas30);
    for kind in [
        SemanticKind::Object(ObjectKind::Server),
        SemanticKind::Object(ObjectKind::ServerVariable),
        SemanticKind::Object(ObjectKind::Operation),
        SemanticKind::Object(ObjectKind::SecurityRequirement),
        SemanticKind::ObjectOrReference(ObjectKind::Parameter),
        SemanticKind::ObjectOrReference(ObjectKind::Schema),
        SemanticKind::ObjectOrReference(ObjectKind::Callback),
        SemanticKind::ObjectOrReference(ObjectKind::Response),
    ] {
        assert!(
            result.ranges.iter().any(|range| range.kind == kind),
            "{kind:?}"
        );
    }
    assert!(
        result
            .ranges
            .iter()
            .any(|range| { range.kind == SemanticKind::Sequence(SequenceKind::Schemas) })
    );
}

#[test]
fn oas31_classifies_webhooks_and_json_schema_2020_12_children() {
    let source = r#"openapi: 3.1.2
info: {title: T, version: v}
paths: {}
webhooks:
  event:
    post:
      responses:
        '204':
          description: ok
components:
  pathItems:
    Shared:
      get:
        responses:
          '200': {description: ok}
  schemas:
    Choice:
      prefixItems:
        - properties:
            id: {type: string}
      dependentSchemas:
        enabled:
          if: {properties: {enabled: {const: true}}}
          then: {required: [value]}
"#;
    let result = classify(source, InputFormat::Yaml).unwrap();
    assert_eq!(result.version, Version::Oas31);
    assert!(
        result
            .ranges
            .iter()
            .any(|range| { range.kind == SemanticKind::Map(MapKind::Webhooks) })
    );
    assert!(
        result
            .ranges
            .iter()
            .any(|range| { range.kind == SemanticKind::Map(MapKind::SchemaDependentSchemas) })
    );
    assert!(
        result
            .ranges
            .iter()
            .any(|range| { range.kind == SemanticKind::Sequence(SequenceKind::Schemas) })
    );
    assert!(
        !result
            .ranges
            .iter()
            .any(|range| { range.kind == SemanticKind::ObjectOrReference(ObjectKind::Schema) })
    );
}

#[test]
fn oas32_classifies_query_additional_operations_and_media_types() {
    let source = r#"openapi: 3.2.0
info: {title: T, version: v}
paths:
  /search:
    query:
      responses:
        '200': {description: ok}
    additionalOperations:
      COPY:
        responses:
          '201': {description: copied}
components:
  mediaTypes:
    stream:
      itemSchema:
        type: string
      prefixEncoding:
        - headers:
            X-Part:
              schema: {type: string}
"#;
    let result = classify(source, InputFormat::Yaml).unwrap();
    assert_eq!(result.version, Version::Oas32);
    assert!(
        result
            .ranges
            .iter()
            .any(|range| { range.kind == SemanticKind::Map(MapKind::AdditionalOperations) })
    );
    assert!(
        result
            .ranges
            .iter()
            .any(|range| { range.kind == SemanticKind::Map(MapKind::ComponentMediaTypes) })
    );
    assert!(
        result
            .ranges
            .iter()
            .any(|range| { range.kind == SemanticKind::Sequence(SequenceKind::Encodings) })
    );
    assert!(has_route(
        source,
        SemanticKind::Object(ObjectKind::Operation),
        &[
            RouteEdge::FixedField("paths".into()),
            RouteEdge::DynamicMapValue("/search".into()),
            RouteEdge::FixedField("query".into()),
        ]
    ));
    assert!(has_route(
        source,
        SemanticKind::Object(ObjectKind::Operation),
        &[
            RouteEdge::FixedField("paths".into()),
            RouteEdge::DynamicMapValue("/search".into()),
            RouteEdge::FixedField("additionalOperations".into()),
            RouteEdge::DynamicMapValue("COPY".into()),
        ]
    ));
}

#[test]
fn json_array_items_use_the_same_neutral_sequence_graph() {
    let source = r#"{"openapi":"3.1.2","info":{"title":"T","version":"v"},"servers":[{"url":"https://example.test","variables":{"port":{"default":"443"}}}],"paths":{}}"#;
    let result = classify(source, InputFormat::Json).unwrap();
    assert!(
        result
            .ranges
            .iter()
            .any(|range| { range.kind == SemanticKind::Sequence(SequenceKind::Servers) })
    );
    assert!(
        result
            .ranges
            .iter()
            .any(|range| { range.kind == SemanticKind::Object(ObjectKind::ServerVariable) })
    );
}

#[test]
fn fixed_operation_methods_with_sequence_values_remain_unclassified_and_unchanged() {
    for (source, input_format) in [
        (
            "openapi: 3.1.2\npaths:\n  /pets:\n    get: [summary, responses]\n",
            InputFormat::Yaml,
        ),
        (
            r#"{"openapi":"3.1.2","paths":{"/pets":{"get":["summary","responses"]}}}"#,
            InputFormat::Json,
        ),
    ] {
        let inventory = classify(source, input_format).unwrap();
        assert!(
            !inventory.ranges.iter().any(|classified| {
                classified.kind == SemanticKind::Object(ObjectKind::Operation)
            })
        );

        let result = format(source, input_format).unwrap();
        assert!(!result.changed);
        assert_eq!(result.output, source);
    }
}

#[test]
fn unknown_response_keys_do_not_create_response_classification_collisions() {
    let source = r#"openapi: 3.1.2
paths:
  /pets:
    get:
      responses:
        typo:
          content:
            application/json:
              schema:
                type: string
"#;
    let inventory = classify(source, InputFormat::Yaml).unwrap();
    assert!(!inventory.ranges.iter().any(|classified| {
        classified.kind == SemanticKind::ObjectOrReference(ObjectKind::Response)
            && classified.route.last() == Some(&RouteEdge::DynamicMapValue("typo".into()))
    }));
    assert!(!inventory.ranges.iter().any(|classified| {
        classified
            .route
            .iter()
            .any(|edge| edge == &RouteEdge::DynamicMapValue("typo".into()))
    }));
}

#[test]
fn context_collisions_terminate_at_opaque_values() {
    let source = r#"openapi: 3.2.0
info: {title: T, version: v}
paths:
  /real:
    get:
      responses:
        '200': {description: ok}
components:
  examples:
    collision:
      value:
        get: {summary: example data}
    dataCollision:
      dataValue:
        get: {summary: example instance data}
  links:
    collision:
      parameters:
        get: {summary: runtime expression data}
  schemas:
    Collision:
      default:
        get: {summary: default data}
      examples:
        - get: {summary: instance data}
      const:
        get: {summary: const instance data}
      enum:
        - get: {summary: enum instance data}
      customVocabulary:
        get: {summary: custom data}
      x-data:
        get: {summary: extension data}
"#;
    let result = classify(source, InputFormat::Yaml).unwrap();
    let operations: Vec<_> = result
        .ranges
        .iter()
        .filter(|range| range.kind == SemanticKind::Object(ObjectKind::Operation))
        .collect();
    assert_eq!(operations.len(), 1);
    let opaque_count = result
        .ranges
        .iter()
        .filter(|range| range.kind == SemanticKind::Opaque)
        .count();
    assert_eq!(opaque_count, 9);
}
