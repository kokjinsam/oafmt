//! Version-specific OpenAPI semantic classification for `oafmt`.

/// The supported OpenAPI minor families.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Version {
    Oas30,
    Oas31,
    Oas32,
}

/// A named Object in the OpenAPI specification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ObjectKind {
    OpenApi,
    Info,
    Contact,
    License,
    Server,
    ServerVariable,
    Components,
    Paths,
    PathItem,
    Operation,
    ExternalDocumentation,
    Parameter,
    RequestBody,
    MediaType,
    Encoding,
    Responses,
    Response,
    Callback,
    Example,
    Link,
    Header,
    Tag,
    Reference,
    Schema,
    Discriminator,
    Xml,
    SecurityScheme,
    OAuthFlows,
    OAuthFlow,
    SecurityRequirement,
}

/// A context-defined mapping whose keys name its values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MapKind {
    Webhooks,
    ServerVariables,
    ComponentSchemas,
    ComponentResponses,
    ComponentParameters,
    ComponentExamples,
    ComponentRequestBodies,
    ComponentHeaders,
    ComponentSecuritySchemes,
    ComponentLinks,
    ComponentCallbacks,
    ComponentPathItems,
    ComponentMediaTypes,
    Content,
    Callbacks,
    Headers,
    Examples,
    Links,
    Encodings,
    AdditionalOperations,
    SchemaProperties,
    SchemaPatternProperties,
    SchemaDefinitions,
    SchemaDependentSchemas,
}

/// A context-defined sequence whose positions contain semantic values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SequenceKind {
    Servers,
    Tags,
    Parameters,
    SecurityRequirements,
    Schemas,
    Encodings,
}

/// The semantic expectation at a syntax value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SemanticKind {
    Object(ObjectKind),
    ObjectOrReference(ObjectKind),
    Map(MapKind),
    Sequence(SequenceKind),
    Opaque,
}

/// The syntax edge used to enter a child value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge<'a> {
    FixedField(&'a str),
    DynamicMapValue(&'a str),
    SequenceItem(usize),
}

/// One table-backed fixed-field transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FixedTransition {
    pub parent: ObjectKind,
    pub field: &'static str,
    pub child: SemanticKind,
}

const ROOT_30: &[&str] = &[
    "openapi",
    "info",
    "servers",
    "paths",
    "components",
    "security",
    "tags",
    "externalDocs",
];
const ROOT_31: &[&str] = &[
    "openapi",
    "info",
    "jsonSchemaDialect",
    "servers",
    "paths",
    "webhooks",
    "components",
    "security",
    "tags",
    "externalDocs",
];
const ROOT_32: &[&str] = &[
    "openapi",
    "$self",
    "info",
    "jsonSchemaDialect",
    "servers",
    "paths",
    "webhooks",
    "components",
    "security",
    "tags",
    "externalDocs",
];
const OPERATION: &[&str] = &[
    "tags",
    "summary",
    "description",
    "externalDocs",
    "operationId",
    "parameters",
    "requestBody",
    "responses",
    "callbacks",
    "deprecated",
    "security",
    "servers",
];

use MapKind as M;
use ObjectKind as O;
use SemanticKind as S;
use SequenceKind as Q;

const COMMON_FIXED: &[FixedTransition] = &[
    fixed(O::OpenApi, "info", S::Object(O::Info)),
    fixed(O::OpenApi, "servers", S::Sequence(Q::Servers)),
    fixed(O::OpenApi, "paths", S::Object(O::Paths)),
    fixed(O::OpenApi, "components", S::Object(O::Components)),
    fixed(O::OpenApi, "security", S::Sequence(Q::SecurityRequirements)),
    fixed(O::OpenApi, "tags", S::Sequence(Q::Tags)),
    fixed(
        O::OpenApi,
        "externalDocs",
        S::Object(O::ExternalDocumentation),
    ),
    fixed(O::Info, "contact", S::Object(O::Contact)),
    fixed(O::Info, "license", S::Object(O::License)),
    fixed(O::Server, "variables", S::Map(M::ServerVariables)),
    fixed(O::Components, "schemas", S::Map(M::ComponentSchemas)),
    fixed(O::Components, "responses", S::Map(M::ComponentResponses)),
    fixed(O::Components, "parameters", S::Map(M::ComponentParameters)),
    fixed(O::Components, "examples", S::Map(M::ComponentExamples)),
    fixed(
        O::Components,
        "requestBodies",
        S::Map(M::ComponentRequestBodies),
    ),
    fixed(O::Components, "headers", S::Map(M::ComponentHeaders)),
    fixed(
        O::Components,
        "securitySchemes",
        S::Map(M::ComponentSecuritySchemes),
    ),
    fixed(O::Components, "links", S::Map(M::ComponentLinks)),
    fixed(O::Components, "callbacks", S::Map(M::ComponentCallbacks)),
    fixed(O::PathItem, "get", S::Object(O::Operation)),
    fixed(O::PathItem, "put", S::Object(O::Operation)),
    fixed(O::PathItem, "post", S::Object(O::Operation)),
    fixed(O::PathItem, "delete", S::Object(O::Operation)),
    fixed(O::PathItem, "options", S::Object(O::Operation)),
    fixed(O::PathItem, "head", S::Object(O::Operation)),
    fixed(O::PathItem, "patch", S::Object(O::Operation)),
    fixed(O::PathItem, "trace", S::Object(O::Operation)),
    fixed(O::PathItem, "servers", S::Sequence(Q::Servers)),
    fixed(O::PathItem, "parameters", S::Sequence(Q::Parameters)),
    fixed(
        O::Operation,
        "externalDocs",
        S::Object(O::ExternalDocumentation),
    ),
    fixed(O::Operation, "parameters", S::Sequence(Q::Parameters)),
    fixed(
        O::Operation,
        "requestBody",
        S::ObjectOrReference(O::RequestBody),
    ),
    fixed(O::Operation, "responses", S::Object(O::Responses)),
    fixed(O::Operation, "callbacks", S::Map(M::Callbacks)),
    fixed(
        O::Operation,
        "security",
        S::Sequence(Q::SecurityRequirements),
    ),
    fixed(O::Operation, "servers", S::Sequence(Q::Servers)),
    fixed(O::Parameter, "schema", S::ObjectOrReference(O::Schema)),
    fixed(O::Parameter, "examples", S::Map(M::Examples)),
    fixed(O::Parameter, "content", S::Map(M::Content)),
    fixed(O::Parameter, "example", S::Opaque),
    fixed(O::RequestBody, "content", S::Map(M::Content)),
    fixed(O::MediaType, "schema", S::ObjectOrReference(O::Schema)),
    fixed(O::MediaType, "example", S::Opaque),
    fixed(O::MediaType, "examples", S::Map(M::Examples)),
    fixed(O::MediaType, "encoding", S::Map(M::Encodings)),
    fixed(O::Encoding, "headers", S::Map(M::Headers)),
    fixed(O::Responses, "default", S::ObjectOrReference(O::Response)),
    fixed(O::Response, "headers", S::Map(M::Headers)),
    fixed(O::Response, "content", S::Map(M::Content)),
    fixed(O::Response, "links", S::Map(M::Links)),
    fixed(O::Example, "value", S::Opaque),
    fixed(O::Link, "parameters", S::Opaque),
    fixed(O::Link, "requestBody", S::Opaque),
    fixed(O::Link, "server", S::Object(O::Server)),
    fixed(O::Header, "schema", S::ObjectOrReference(O::Schema)),
    fixed(O::Header, "examples", S::Map(M::Examples)),
    fixed(O::Header, "content", S::Map(M::Content)),
    fixed(O::Header, "example", S::Opaque),
    fixed(O::Tag, "externalDocs", S::Object(O::ExternalDocumentation)),
    fixed(O::Schema, "properties", S::Map(M::SchemaProperties)),
    fixed(O::Schema, "allOf", S::Sequence(Q::Schemas)),
    fixed(O::Schema, "oneOf", S::Sequence(Q::Schemas)),
    fixed(O::Schema, "anyOf", S::Sequence(Q::Schemas)),
    fixed(O::Schema, "not", S::ObjectOrReference(O::Schema)),
    fixed(O::Schema, "items", S::ObjectOrReference(O::Schema)),
    fixed(
        O::Schema,
        "additionalProperties",
        S::ObjectOrReference(O::Schema),
    ),
    fixed(O::Schema, "discriminator", S::Object(O::Discriminator)),
    fixed(O::Schema, "xml", S::Object(O::Xml)),
    fixed(
        O::Schema,
        "externalDocs",
        S::Object(O::ExternalDocumentation),
    ),
    fixed(O::Schema, "default", S::Opaque),
    fixed(O::Schema, "enum", S::Opaque),
    fixed(O::Schema, "example", S::Opaque),
    fixed(O::Discriminator, "mapping", S::Opaque),
    fixed(O::SecurityScheme, "flows", S::Object(O::OAuthFlows)),
    fixed(O::OAuthFlows, "implicit", S::Object(O::OAuthFlow)),
    fixed(O::OAuthFlows, "password", S::Object(O::OAuthFlow)),
    fixed(O::OAuthFlows, "clientCredentials", S::Object(O::OAuthFlow)),
    fixed(O::OAuthFlows, "authorizationCode", S::Object(O::OAuthFlow)),
    fixed(O::OAuthFlow, "scopes", S::Opaque),
];

const SINCE_31_FIXED: &[FixedTransition] = &[
    fixed(O::OpenApi, "webhooks", S::Map(M::Webhooks)),
    fixed(O::Components, "pathItems", S::Map(M::ComponentPathItems)),
    fixed(O::Schema, "$defs", S::Map(M::SchemaDefinitions)),
    fixed(
        O::Schema,
        "patternProperties",
        S::Map(M::SchemaPatternProperties),
    ),
    fixed(
        O::Schema,
        "dependentSchemas",
        S::Map(M::SchemaDependentSchemas),
    ),
    fixed(O::Schema, "prefixItems", S::Sequence(Q::Schemas)),
    fixed(O::Schema, "contains", S::ObjectOrReference(O::Schema)),
    fixed(O::Schema, "propertyNames", S::ObjectOrReference(O::Schema)),
    fixed(O::Schema, "if", S::ObjectOrReference(O::Schema)),
    fixed(O::Schema, "then", S::ObjectOrReference(O::Schema)),
    fixed(O::Schema, "else", S::ObjectOrReference(O::Schema)),
    fixed(
        O::Schema,
        "unevaluatedItems",
        S::ObjectOrReference(O::Schema),
    ),
    fixed(
        O::Schema,
        "unevaluatedProperties",
        S::ObjectOrReference(O::Schema),
    ),
    fixed(O::Schema, "contentSchema", S::ObjectOrReference(O::Schema)),
    fixed(O::Schema, "const", S::Opaque),
    fixed(O::Schema, "examples", S::Opaque),
];

const ONLY_32_FIXED: &[FixedTransition] = &[
    fixed(O::Components, "mediaTypes", S::Map(M::ComponentMediaTypes)),
    fixed(O::PathItem, "query", S::Object(O::Operation)),
    fixed(
        O::PathItem,
        "additionalOperations",
        S::Map(M::AdditionalOperations),
    ),
    fixed(O::MediaType, "itemSchema", S::ObjectOrReference(O::Schema)),
    fixed(O::MediaType, "prefixEncoding", S::Sequence(Q::Encodings)),
    fixed(O::MediaType, "itemEncoding", S::Object(O::Encoding)),
    fixed(O::Encoding, "encoding", S::Map(M::Encodings)),
    fixed(O::Encoding, "prefixEncoding", S::Sequence(Q::Encodings)),
    fixed(O::Encoding, "itemEncoding", S::Object(O::Encoding)),
    fixed(
        O::OAuthFlows,
        "deviceAuthorization",
        S::Object(O::OAuthFlow),
    ),
    fixed(O::Example, "dataValue", S::Opaque),
    fixed(O::Example, "serializedValue", S::Opaque),
];

const fn fixed(parent: ObjectKind, field: &'static str, child: SemanticKind) -> FixedTransition {
    FixedTransition {
        parent,
        field,
        child,
    }
}

impl Version {
    /// Parse a complete supported `openapi` version string.
    pub fn parse(value: &str) -> Option<Self> {
        let mut parts = value.split('.');
        let major = parts.next()?;
        let minor = parts.next()?;
        let patch = parts.next()?;
        if parts.next().is_some()
            || major != "3"
            || patch.is_empty()
            || !patch.bytes().all(|byte| byte.is_ascii_digit())
        {
            return None;
        }
        match minor {
            "0" => Some(Self::Oas30),
            "1" => Some(Self::Oas31),
            "2" => Some(Self::Oas32),
            _ => None,
        }
    }

    /// Fixed-field order for the entry document root.
    pub fn root_order(self) -> &'static [&'static str] {
        match self {
            Self::Oas30 => ROOT_30,
            Self::Oas31 => ROOT_31,
            Self::Oas32 => ROOT_32,
        }
    }

    /// Fixed-field order for an Operation Object.
    pub fn operation_order(self) -> &'static [&'static str] {
        OPERATION
    }

    /// All fixed-field transitions active for this feature family.
    pub fn fixed_transitions(self) -> impl Iterator<Item = FixedTransition> {
        COMMON_FIXED
            .iter()
            .chain(
                (self != Self::Oas30)
                    .then_some(SINCE_31_FIXED)
                    .into_iter()
                    .flatten(),
            )
            .chain(
                (self == Self::Oas32)
                    .then_some(ONLY_32_FIXED)
                    .into_iter()
                    .flatten(),
            )
            .copied()
            .map(move |mut transition| {
                transition.child = self.adjust_schema_expectation(transition.child);
                transition
            })
    }

    /// Classify a child from its parent expectation and the kind of syntax edge.
    pub fn transition(self, parent: SemanticKind, edge: Edge<'_>) -> Option<SemanticKind> {
        match (parent, edge) {
            (S::Object(object) | S::ObjectOrReference(object), Edge::FixedField(field)) => {
                self.object_field(object, field)
            }
            (S::Object(object) | S::ObjectOrReference(object), Edge::DynamicMapValue(key)) => {
                patterned_object_value(object, key)
            }
            (S::Map(map), Edge::DynamicMapValue(_)) => self.map_value(map),
            (S::Sequence(sequence), Edge::SequenceItem(_)) => self.sequence_item(sequence),
            _ => None,
        }
    }

    fn object_field(self, object: ObjectKind, field: &str) -> Option<SemanticKind> {
        self.fixed_transitions()
            .find(|transition| transition.parent == object && transition.field == field)
            .map(|transition| transition.child)
            .or_else(|| field.starts_with("x-").then_some(S::Opaque))
            .or_else(|| (object == O::Schema).then_some(S::Opaque))
    }

    fn adjust_schema_expectation(self, kind: SemanticKind) -> SemanticKind {
        if self != Self::Oas30 && kind == S::ObjectOrReference(O::Schema) {
            S::Object(O::Schema)
        } else {
            kind
        }
    }

    fn map_value(self, map: MapKind) -> Option<SemanticKind> {
        let child = match map {
            M::Webhooks | M::ComponentPathItems if self != Self::Oas30 => S::Object(O::PathItem),
            M::Webhooks | M::ComponentPathItems => return None,
            M::ServerVariables => S::Object(O::ServerVariable),
            M::ComponentSchemas
            | M::SchemaProperties
            | M::SchemaPatternProperties
            | M::SchemaDefinitions
            | M::SchemaDependentSchemas => {
                if matches!(
                    map,
                    M::SchemaPatternProperties | M::SchemaDefinitions | M::SchemaDependentSchemas
                ) && self == Self::Oas30
                {
                    return None;
                }
                self.adjust_schema_expectation(S::ObjectOrReference(O::Schema))
            }
            M::ComponentResponses => S::ObjectOrReference(O::Response),
            M::ComponentParameters => S::ObjectOrReference(O::Parameter),
            M::ComponentExamples | M::Examples => S::ObjectOrReference(O::Example),
            M::ComponentRequestBodies => S::ObjectOrReference(O::RequestBody),
            M::ComponentHeaders | M::Headers => S::ObjectOrReference(O::Header),
            M::ComponentSecuritySchemes => S::ObjectOrReference(O::SecurityScheme),
            M::ComponentLinks | M::Links => S::ObjectOrReference(O::Link),
            M::ComponentCallbacks | M::Callbacks => S::ObjectOrReference(O::Callback),
            M::ComponentMediaTypes if self == Self::Oas32 => S::ObjectOrReference(O::MediaType),
            M::ComponentMediaTypes => return None,
            M::Content if self == Self::Oas32 => S::ObjectOrReference(O::MediaType),
            M::Content => S::Object(O::MediaType),
            M::Encodings => S::Object(O::Encoding),
            M::AdditionalOperations if self == Self::Oas32 => S::Object(O::Operation),
            M::AdditionalOperations => return None,
        };
        Some(child)
    }

    fn sequence_item(self, sequence: SequenceKind) -> Option<SemanticKind> {
        let child = match sequence {
            Q::Servers => S::Object(O::Server),
            Q::Tags => S::Object(O::Tag),
            Q::Parameters => S::ObjectOrReference(O::Parameter),
            Q::SecurityRequirements => S::Object(O::SecurityRequirement),
            Q::Schemas => self.adjust_schema_expectation(S::ObjectOrReference(O::Schema)),
            Q::Encodings if self == Self::Oas32 => S::Object(O::Encoding),
            Q::Encodings => return None,
        };
        Some(child)
    }
}

fn patterned_object_value(object: ObjectKind, key: &str) -> Option<SemanticKind> {
    match object {
        O::Paths if key.starts_with('/') => Some(S::Object(O::PathItem)),
        O::Responses if is_response_key(key) => Some(S::ObjectOrReference(O::Response)),
        O::Callback => Some(S::Object(O::PathItem)),
        O::SecurityRequirement => Some(S::Opaque),
        _ => None,
    }
}

fn is_response_key(key: &str) -> bool {
    matches!(
        key.as_bytes(),
        [b'1'..=b'5', b'0'..=b'9', b'0'..=b'9'] | [b'1'..=b'5', b'X', b'X']
    )
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{
        Edge, MapKind, ONLY_32_FIXED, ObjectKind, SINCE_31_FIXED, SemanticKind, SequenceKind,
        Version,
    };

    #[test]
    fn ordering_tables_remain_frozen() {
        assert_eq!(
            Version::Oas30.root_order(),
            [
                "openapi",
                "info",
                "servers",
                "paths",
                "components",
                "security",
                "tags",
                "externalDocs"
            ]
        );
        assert_eq!(
            Version::Oas31.root_order(),
            [
                "openapi",
                "info",
                "jsonSchemaDialect",
                "servers",
                "paths",
                "webhooks",
                "components",
                "security",
                "tags",
                "externalDocs"
            ]
        );
        assert_eq!(
            Version::Oas32.root_order(),
            [
                "openapi",
                "$self",
                "info",
                "jsonSchemaDialect",
                "servers",
                "paths",
                "webhooks",
                "components",
                "security",
                "tags",
                "externalDocs"
            ]
        );
        assert_eq!(
            Version::Oas30.operation_order(),
            [
                "tags",
                "summary",
                "description",
                "externalDocs",
                "operationId",
                "parameters",
                "requestBody",
                "responses",
                "callbacks",
                "deprecated",
                "security",
                "servers"
            ]
        );
    }

    #[test]
    fn transition_tables_have_no_duplicate_or_contradictory_edges() {
        for version in [Version::Oas30, Version::Oas31, Version::Oas32] {
            let mut edges = BTreeSet::new();
            for transition in version.fixed_transitions() {
                assert!(
                    edges.insert((transition.parent, transition.field)),
                    "{version:?} duplicate {:?}.{}",
                    transition.parent,
                    transition.field
                );
                assert_eq!(
                    version.transition(
                        SemanticKind::Object(transition.parent),
                        Edge::FixedField(transition.field)
                    ),
                    Some(transition.child)
                );
            }
        }
    }

    #[test]
    fn version_deltas_and_reference_unions_are_explicit() {
        let path_item = SemanticKind::Object(ObjectKind::PathItem);
        assert_eq!(
            Version::Oas32.transition(path_item, Edge::FixedField("query")),
            Some(SemanticKind::Object(ObjectKind::Operation))
        );
        assert_eq!(
            Version::Oas31.transition(path_item, Edge::FixedField("query")),
            None
        );
        assert_eq!(
            Version::Oas31.transition(
                SemanticKind::Object(ObjectKind::OpenApi),
                Edge::FixedField("webhooks")
            ),
            Some(SemanticKind::Map(MapKind::Webhooks))
        );
        assert_eq!(
            Version::Oas30.transition(
                SemanticKind::Object(ObjectKind::OpenApi),
                Edge::FixedField("webhooks")
            ),
            None
        );
        assert_eq!(
            Version::Oas32.transition(
                SemanticKind::Map(MapKind::ComponentMediaTypes),
                Edge::DynamicMapValue("json")
            ),
            Some(SemanticKind::ObjectOrReference(ObjectKind::MediaType))
        );
        assert_eq!(
            Version::Oas30.transition(
                SemanticKind::Map(MapKind::ComponentSchemas),
                Edge::DynamicMapValue("Pet")
            ),
            Some(SemanticKind::ObjectOrReference(ObjectKind::Schema))
        );
        assert_eq!(
            Version::Oas31.transition(
                SemanticKind::Map(MapKind::ComponentSchemas),
                Edge::DynamicMapValue("Pet")
            ),
            Some(SemanticKind::Object(ObjectKind::Schema))
        );
        assert_eq!(
            Version::Oas30.transition(
                SemanticKind::Sequence(SequenceKind::Parameters),
                Edge::SequenceItem(0)
            ),
            Some(SemanticKind::ObjectOrReference(ObjectKind::Parameter))
        );
        assert_eq!(
            Version::Oas31.transition(
                SemanticKind::Map(MapKind::ComponentMediaTypes),
                Edge::DynamicMapValue("json")
            ),
            None
        );
        assert_eq!(
            Version::Oas31.transition(
                SemanticKind::Sequence(SequenceKind::Encodings),
                Edge::SequenceItem(0)
            ),
            None
        );
    }

    #[test]
    fn every_version_delta_is_absent_from_earlier_families() {
        for transition in SINCE_31_FIXED {
            assert!(
                !Version::Oas30.fixed_transitions().any(|earlier| {
                    earlier.parent == transition.parent && earlier.field == transition.field
                }),
                "3.1 edge {:?}.{}",
                transition.parent,
                transition.field
            );
        }
        for transition in ONLY_32_FIXED {
            assert!(
                !Version::Oas31.fixed_transitions().any(|earlier| {
                    earlier.parent == transition.parent && earlier.field == transition.field
                }),
                "3.2 edge {:?}.{}",
                transition.parent,
                transition.field
            );
        }
    }

    #[test]
    fn every_container_transition_uses_its_declared_edge_kind() {
        for (version, map) in [
            (Version::Oas31, MapKind::Webhooks),
            (Version::Oas30, MapKind::ServerVariables),
            (Version::Oas30, MapKind::ComponentSchemas),
            (Version::Oas30, MapKind::ComponentResponses),
            (Version::Oas30, MapKind::ComponentParameters),
            (Version::Oas30, MapKind::ComponentExamples),
            (Version::Oas30, MapKind::ComponentRequestBodies),
            (Version::Oas30, MapKind::ComponentHeaders),
            (Version::Oas30, MapKind::ComponentSecuritySchemes),
            (Version::Oas30, MapKind::ComponentLinks),
            (Version::Oas30, MapKind::ComponentCallbacks),
            (Version::Oas31, MapKind::ComponentPathItems),
            (Version::Oas32, MapKind::ComponentMediaTypes),
            (Version::Oas30, MapKind::Content),
            (Version::Oas30, MapKind::Callbacks),
            (Version::Oas30, MapKind::Headers),
            (Version::Oas30, MapKind::Examples),
            (Version::Oas30, MapKind::Links),
            (Version::Oas30, MapKind::Encodings),
            (Version::Oas32, MapKind::AdditionalOperations),
            (Version::Oas30, MapKind::SchemaProperties),
            (Version::Oas31, MapKind::SchemaPatternProperties),
            (Version::Oas31, MapKind::SchemaDefinitions),
            (Version::Oas31, MapKind::SchemaDependentSchemas),
        ] {
            let parent = SemanticKind::Map(map);
            assert!(
                version
                    .transition(parent, Edge::DynamicMapValue("key"))
                    .is_some(),
                "{version:?} {map:?}"
            );
            assert_eq!(version.transition(parent, Edge::FixedField("key")), None);
            assert_eq!(version.transition(parent, Edge::SequenceItem(0)), None);
        }

        for (version, sequence) in [
            (Version::Oas30, SequenceKind::Servers),
            (Version::Oas30, SequenceKind::Tags),
            (Version::Oas30, SequenceKind::Parameters),
            (Version::Oas30, SequenceKind::SecurityRequirements),
            (Version::Oas30, SequenceKind::Schemas),
            (Version::Oas32, SequenceKind::Encodings),
        ] {
            let parent = SemanticKind::Sequence(sequence);
            assert!(
                version.transition(parent, Edge::SequenceItem(0)).is_some(),
                "{version:?} {sequence:?}"
            );
            assert_eq!(version.transition(parent, Edge::FixedField("key")), None);
            assert_eq!(
                version.transition(parent, Edge::DynamicMapValue("key")),
                None
            );
        }
    }

    #[test]
    fn patterned_objects_are_contextual_dynamic_edges() {
        for (object, key, expected) in [
            (ObjectKind::Paths, "/pets", ObjectKind::PathItem),
            (ObjectKind::Responses, "200", ObjectKind::Response),
            (
                ObjectKind::Callback,
                "{$request.body#/url}",
                ObjectKind::PathItem,
            ),
        ] {
            assert!(matches!(
                Version::Oas32.transition(
                    SemanticKind::Object(object),
                    Edge::DynamicMapValue(key)
                ),
                Some(SemanticKind::Object(actual))
                    | Some(SemanticKind::ObjectOrReference(actual))
                    if actual == expected
            ));
        }
        assert_eq!(
            Version::Oas32.transition(
                SemanticKind::Object(ObjectKind::Paths),
                Edge::DynamicMapValue("not-a-path")
            ),
            None
        );
        assert_eq!(
            Version::Oas32.transition(
                SemanticKind::Object(ObjectKind::SecurityRequirement),
                Edge::DynamicMapValue("oauth")
            ),
            Some(SemanticKind::Opaque)
        );
    }

    #[test]
    fn responses_only_classify_supported_status_code_patterns() {
        let responses = SemanticKind::Object(ObjectKind::Responses);
        let response = Some(SemanticKind::ObjectOrReference(ObjectKind::Response));

        for version in [Version::Oas30, Version::Oas31, Version::Oas32] {
            for key in ["100", "204", "418", "500", "1XX", "5XX"] {
                assert_eq!(
                    version.transition(responses, Edge::DynamicMapValue(key)),
                    response,
                    "{version:?} {key}"
                );
            }
            for key in ["typo", "600", "2xx", "099", "20", "2000", "X-XX"] {
                assert_eq!(
                    version.transition(responses, Edge::DynamicMapValue(key)),
                    None,
                    "{version:?} {key}"
                );
            }
            assert_eq!(
                version.transition(responses, Edge::FixedField("default")),
                response
            );
            assert_eq!(
                version.transition(responses, Edge::FixedField("x-data")),
                Some(SemanticKind::Opaque)
            );
        }
    }

    #[test]
    fn opaque_contexts_stop_key_collisions() {
        for (object, field) in [
            (ObjectKind::Example, "value"),
            (ObjectKind::Schema, "default"),
            (ObjectKind::Link, "parameters"),
            (ObjectKind::Operation, "x-data"),
            (ObjectKind::Schema, "customKeyword"),
        ] {
            assert_eq!(
                Version::Oas32.transition(SemanticKind::Object(object), Edge::FixedField(field)),
                Some(SemanticKind::Opaque)
            );
        }
        assert_eq!(
            Version::Oas32.transition(SemanticKind::Opaque, Edge::FixedField("get")),
            None
        );
    }

    #[test]
    fn only_complete_supported_versions_parse() {
        assert_eq!(Version::parse("3.0.0"), Some(Version::Oas30));
        assert_eq!(Version::parse("3.1.12"), Some(Version::Oas31));
        assert_eq!(Version::parse("3.2.0"), Some(Version::Oas32));
        for value in ["", "3", "3.0", "3.3.0", "2.0.0", "3.1.x", "3.1.0.1"] {
            assert_eq!(Version::parse(value), None, "{value}");
        }
    }
}
