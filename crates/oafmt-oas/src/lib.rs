//! Minimal OpenAPI ordering and location classification for `oafmt`.

/// The supported OpenAPI minor families.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Version {
    Oas30,
    Oas31,
    Oas32,
}

/// A mapping location relevant to Phase 2 formatting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Location {
    Root,
    Paths,
    PathItem,
    Operation,
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

    /// Classify a child mapping only from its parent location and entry key.
    pub fn classify_child(self, parent: Location, key: &str) -> Option<Location> {
        match parent {
            Location::Root if key == "paths" => Some(Location::Paths),
            Location::Paths if key.starts_with('/') => Some(Location::PathItem),
            Location::PathItem if self.is_operation_method(key) => Some(Location::Operation),
            _ => None,
        }
    }

    fn is_operation_method(self, key: &str) -> bool {
        matches!(
            key,
            "get" | "put" | "post" | "delete" | "options" | "head" | "patch" | "trace"
        ) || (self == Self::Oas32 && key == "query")
    }
}

#[cfg(test)]
mod tests {
    use super::{Location, Version};

    #[test]
    fn version_tables_and_location_classifier_are_frozen() {
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

        for version in [Version::Oas30, Version::Oas31, Version::Oas32] {
            assert_eq!(
                version.classify_child(Location::Root, "paths"),
                Some(Location::Paths)
            );
            assert_eq!(
                version.classify_child(Location::Paths, "/pets"),
                Some(Location::PathItem)
            );
            assert_eq!(
                version.classify_child(Location::PathItem, "get"),
                Some(Location::Operation)
            );
            assert_eq!(version.classify_child(Location::Root, "get"), None);
            assert_eq!(version.classify_child(Location::Paths, "get"), None);
            assert_eq!(version.classify_child(Location::Root, "webhooks"), None);
        }
        assert_eq!(
            Version::Oas32.classify_child(Location::PathItem, "query"),
            Some(Location::Operation)
        );
        assert_eq!(
            Version::Oas31.classify_child(Location::PathItem, "query"),
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
