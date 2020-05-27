use crate::{DynamicProperties, Properties, Property, PropertyType, PropertyTypeRegistry};
use de::SeqAccess;
use serde::{
    de::{self, DeserializeSeed, MapAccess, Visitor},
    ser::{SerializeMap, SerializeSeq},
    Serialize,
};

pub const TYPE_FIELD: &str = "type";
pub const MAP_FIELD: &str = "map";
pub const SEQ_FIELD: &str = "seq";
pub const VALUE_FIELD: &str = "value";

pub enum Serializable<'a> {
    Owned(Box<dyn erased_serde::Serialize + 'a>),
    Borrowed(&'a dyn erased_serde::Serialize),
}

impl<'a> Serializable<'a> {
    pub fn borrow(&self) -> &dyn erased_serde::Serialize {
        match self {
            Serializable::Borrowed(serialize) => serialize,
            Serializable::Owned(serialize) => serialize,
        }
    }
}
pub struct PropertyValueSerializer<'a, T>
where
    T: Property + Serialize,
{
    pub property: &'a T,
}

impl<'a, T> PropertyValueSerializer<'a, T>
where
    T: Property + Serialize,
{
    pub fn new(property: &'a T) -> Self {
        PropertyValueSerializer { property }
    }
}

impl<'a, T> Serialize for PropertyValueSerializer<'a, T>
where
    T: Property + Serialize,
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut state = serializer.serialize_map(Some(2))?;
        state.serialize_entry(TYPE_FIELD, self.property.type_name())?;
        state.serialize_entry(VALUE_FIELD, self.property)?;
        state.end()
    }
}

impl Serialize for DynamicProperties {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self.property_type {
            PropertyType::Map => MapSerializer::new(self).serialize(serializer),
            PropertyType::Seq => SeqSerializer::new(self).serialize(serializer),
            _ => {
                return Err(serde::ser::Error::custom(
                    "DynamicProperties cannot be Value type",
                ))
            }
        }
    }
}

pub struct MapSerializer<'a> {
    pub properties: &'a dyn Properties,
}

impl<'a> MapSerializer<'a> {
    pub fn new(properties: &'a dyn Properties) -> Self {
        MapSerializer { properties }
    }
}

impl<'a> Serialize for MapSerializer<'a> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut state = serializer.serialize_map(Some(2))?;
        state.serialize_entry(TYPE_FIELD, self.properties.type_name())?;
        state.serialize_entry(
            MAP_FIELD,
            &MapValueSerializer {
                properties: self.properties,
            },
        )?;
        state.end()
    }
}

pub struct MapValueSerializer<'a> {
    pub properties: &'a dyn Properties,
}

impl<'a> Serialize for MapValueSerializer<'a> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut state = serializer.serialize_map(Some(self.properties.prop_len()))?;
        for (index, property) in self.properties.iter_props().enumerate() {
            let name = self.properties.prop_name(index).unwrap();
            state.serialize_entry(name, property.serializable().borrow())?;
        }
        state.end()
    }
}

pub struct SeqSerializer<'a> {
    pub properties: &'a dyn Properties,
}

impl<'a> SeqSerializer<'a> {
    pub fn new(properties: &'a dyn Properties) -> Self {
        SeqSerializer { properties }
    }
}

impl<'a> Serialize for SeqSerializer<'a> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut state = serializer.serialize_map(Some(2))?;
        state.serialize_entry(TYPE_FIELD, self.properties.type_name())?;
        state.serialize_entry(
            SEQ_FIELD,
            &SeqValueSerializer {
                properties: self.properties,
            },
        )?;
        state.end()
    }
}

pub struct SeqValueSerializer<'a> {
    pub properties: &'a dyn Properties,
}

impl<'a> Serialize for SeqValueSerializer<'a> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut state = serializer.serialize_seq(Some(self.properties.prop_len()))?;
        for prop in self.properties.iter_props() {
            state.serialize_element(prop.serializable().borrow())?;
        }
        state.end()
    }
}

pub struct PropertyDeserializer<'a> {
    type_name: Option<&'a str>,
    registry: &'a PropertyTypeRegistry,
}

impl<'a, 'de> DeserializeSeed<'de> for PropertyDeserializer<'a> {
    type Value = Box<dyn Property>;
    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        if let Some(type_name) = self.type_name {
            let registration = self.registry.get_short(type_name).ok_or_else(|| {
                de::Error::custom(format!("TypeRegistration is missing for {}", type_name))
            })?;
            let mut erased = erased_serde::Deserializer::erase(deserializer);
            (registration.deserialize)(&mut erased, self.registry)
                .map_err(<<D as serde::Deserializer<'de>>::Error as serde::de::Error>::custom)
        } else {
            deserializer.deserialize_any(AnyPropVisiter {
                registry: self.registry,
            })
        }
    }
}
pub struct SeqPropertyDeserializer<'a> {
    registry: &'a PropertyTypeRegistry,
}

impl<'a, 'de> DeserializeSeed<'de> for SeqPropertyDeserializer<'a> {
    type Value = DynamicProperties;
    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_seq(SeqPropertyVisiter {
            registry: self.registry,
        })
    }
}

pub struct SeqPropertyVisiter<'a> {
    registry: &'a PropertyTypeRegistry,
}

impl<'a, 'de> Visitor<'de> for SeqPropertyVisiter<'a> {
    type Value = DynamicProperties;
    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("property value")
    }

    fn visit_seq<V>(self, mut seq: V) -> Result<Self::Value, V::Error>
    where
        V: SeqAccess<'de>,
    {
        let mut dynamic_properties = DynamicProperties::seq();
        while let Some(prop) = seq.next_element_seed(PropertyDeserializer {
            registry: self.registry,
            type_name: None,
        })? {
            dynamic_properties.push(prop, None);
        }
        Ok(dynamic_properties)
    }
}

pub struct MapPropertyDeserializer<'a> {
    registry: &'a PropertyTypeRegistry,
}

impl<'a> MapPropertyDeserializer<'a> {
    pub fn new(registry: &'a PropertyTypeRegistry) -> Self {
        MapPropertyDeserializer {
            registry,
        }
    }
}

impl<'a, 'de> DeserializeSeed<'de> for MapPropertyDeserializer<'a> {
    type Value = DynamicProperties;
    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_map(MapPropertyVisiter {
            registry: self.registry,
        })
    }
}

struct MapPropertyVisiter<'a> {
    registry: &'a PropertyTypeRegistry,
}

impl<'a, 'de> Visitor<'de> for MapPropertyVisiter<'a> {
    type Value = DynamicProperties;
    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("map value")
    }

    fn visit_map<V>(self, mut map: V) -> Result<Self::Value, V::Error>
    where
        V: MapAccess<'de>,
    {
        let mut dynamic_properties = DynamicProperties::map();
        while let Some(key) = map.next_key::<String>()? {
            let property = map.next_value_seed(PropertyDeserializer {
                registry: self.registry,
                type_name: None,
            })?;
            dynamic_properties.set_box(&key, property);
        }

        Ok(dynamic_properties)
    }
}

struct AnyPropVisiter<'a> {
    registry: &'a PropertyTypeRegistry,
}

impl<'a, 'de> Visitor<'de> for AnyPropVisiter<'a> {
    type Value = Box<dyn Property>;
    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("property value")
    }

    fn visit_u8<E>(self, v: u8) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(Box::new(v))
    }

    fn visit_bool<E>(self, v: bool) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(Box::new(v))
    }

    fn visit_u16<E>(self, v: u16) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(Box::new(v))
    }

    fn visit_u32<E>(self, v: u32) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(Box::new(v))
    }

    fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(Box::new(v))
    }

    fn visit_i8<E>(self, v: i8) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(Box::new(v))
    }

    fn visit_i16<E>(self, v: i16) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(Box::new(v))
    }

    fn visit_i32<E>(self, v: i32) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(Box::new(v))
    }

    fn visit_i64<E>(self, v: i64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(Box::new(v))
    }

    fn visit_f32<E>(self, v: f32) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(Box::new(v))
    }

    fn visit_f64<E>(self, v: f64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(Box::new(v))
    }

    fn visit_string<E>(self, v: String) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(Box::new(v))
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(Box::new(v.to_string()))
    }

    fn visit_map<V>(self, mut map: V) -> Result<Self::Value, V::Error>
    where
        V: MapAccess<'de>,
    {
        let mut type_name: Option<String> = None;
        while let Some(key) = map.next_key::<String>()? {
            match key.as_str() {
                TYPE_FIELD => {
                    type_name = Some(map.next_value()?);
                }
                MAP_FIELD => {
                    let type_name = type_name
                        .take()
                        .ok_or_else(|| de::Error::missing_field(TYPE_FIELD))?;
                    let mut dynamic_properties =
                        map.next_value_seed(MapPropertyDeserializer { registry: self.registry })?;
                    dynamic_properties.type_name = type_name.to_string();
                    return Ok(Box::new(
                        dynamic_properties,
                    ));
                }
                SEQ_FIELD => {
                    let type_name = type_name
                        .take()
                        .ok_or_else(|| de::Error::missing_field(TYPE_FIELD))?;
                    let mut dynamic_properties =
                        map.next_value_seed(SeqPropertyDeserializer { registry: self.registry })?;
                    dynamic_properties.type_name = type_name;
                    return Ok(Box::new(
                        dynamic_properties,
                    ));
                }
                VALUE_FIELD => {
                    let type_name = type_name
                        .take()
                        .ok_or_else(|| de::Error::missing_field(TYPE_FIELD))?;
                    return map.next_value_seed(
                        PropertyDeserializer {
                            registry: self.registry,
                            type_name: Some(&type_name),
                        },
                    );
                }
                _ => return Err(de::Error::unknown_field(key.as_str(), &[])),
            }
        }

        Err(de::Error::custom("Maps in this location must have the \'type\' field and one of the following fields: \'map\', \'seq\', \'value\'"))
    }
}