//! String-keyed maps whose key pattern is part of the JSON Schema.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fmt;
use std::marker::PhantomData;

use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::de::{Deserialize, Deserializer, MapAccess, Visitor};
use serde::ser::{Serialize, SerializeMap, Serializer};
use utoipa::ToSchema;
use utoipa::openapi::schema::{ObjectBuilder, Type};

use crate::schema_util::schema_object;

pub trait PatternKey: Sized {
    const PATTERN: &'static str;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatternMap<K, V> {
    pub entries: BTreeMap<K, V>,
}

impl<K, V> Default for PatternMap<K, V> {
    fn default() -> Self {
        Self {
            entries: BTreeMap::new(),
        }
    }
}

impl<K, V> PatternMap<K, V> {
    pub fn get(&self, key: &K) -> Option<&V>
    where
        K: Ord,
    {
        self.entries.get(key)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&K, &V)> {
        self.entries.iter()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl<K: Serialize + Ord, V: Serialize> Serialize for PatternMap<K, V> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.entries.len()))?;
        for (key, value) in &self.entries {
            map.serialize_entry(key, value)?;
        }
        map.end()
    }
}

impl<'de, K, V> Deserialize<'de> for PatternMap<K, V>
where
    K: Deserialize<'de> + Ord,
    V: Deserialize<'de>,
{
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct MapVisitor<K, V>(PhantomData<(K, V)>);
        impl<'de, K, V> Visitor<'de> for MapVisitor<K, V>
        where
            K: Deserialize<'de> + Ord,
            V: Deserialize<'de>,
        {
            type Value = PatternMap<K, V>;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a map with patterned keys")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut access: A) -> Result<Self::Value, A::Error> {
                let mut entries = BTreeMap::new();
                while let Some((key, value)) = access.next_entry::<K, V>()? {
                    if entries.insert(key, value).is_some() {
                        return Err(serde::de::Error::custom("duplicate map key"));
                    }
                }
                Ok(PatternMap { entries })
            }
        }
        deserializer.deserialize_map(MapVisitor(PhantomData))
    }
}

impl<K: PatternKey, V: JsonSchema> JsonSchema for PatternMap<K, V> {
    fn schema_name() -> Cow<'static, str> {
        Cow::Owned(format!(
            "PatternMap_{}_{}",
            std::any::type_name::<K>(),
            V::schema_name()
        ))
    }

    fn schema_id() -> Cow<'static, str> {
        Cow::Owned(format!(
            "PatternMap<{}><{}>",
            std::any::type_name::<K>(),
            V::schema_id()
        ))
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let additional = generator.subschema_for::<V>();
        let mut additional = additional.to_value();
        if let Some(object) = additional.as_object_mut() {
            object.remove("title");
        }
        schema_object(serde_json::json!({
            "type": "object",
            "additionalProperties": additional,
            "propertyNames": {
                "type": "string",
                "pattern": K::PATTERN
            }
        }))
    }
}

impl<K: PatternKey, V: utoipa::PartialSchema> utoipa::__dev::ComposeSchema for PatternMap<K, V> {
    fn compose(
        schemas: Vec<utoipa::openapi::RefOr<utoipa::openapi::schema::Schema>>,
    ) -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        let names = schemas.first().cloned().unwrap_or_else(|| {
            ObjectBuilder::new()
                .schema_type(Type::String)
                .pattern(Some(K::PATTERN))
                .into()
        });
        let values = schemas.get(1).cloned().unwrap_or_else(V::schema);
        ObjectBuilder::new()
            .schema_type(Type::Object)
            .additional_properties(Some(values))
            .property_names(Some(names))
            .into()
    }
}

impl<K: PatternKey, V: ToSchema> ToSchema for PatternMap<K, V> {
    fn name() -> Cow<'static, str> {
        Cow::Owned(format!("PatternMap{}", V::name()))
    }

    fn schemas(
        schemas: &mut Vec<(
            String,
            utoipa::openapi::RefOr<utoipa::openapi::schema::Schema>,
        )>,
    ) {
        V::schemas(schemas);
    }
}
