use super::*;

use crate::{semantic_analysis::*, types::*, CallPath, Ident, TypeArgument, TypeParameter};

use sway_types::{span::Span, Spanned};

use derivative::Derivative;
use std::{
    fmt,
    hash::{Hash, Hasher},
};

#[derive(Debug, Clone, Hash, PartialEq)]
pub enum AbiName {
    Deferred,
    Known(CallPath),
}

impl fmt::Display for AbiName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(
            &(match self {
                AbiName::Deferred => "for unspecified ABI".to_string(),
                AbiName::Known(cp) => cp.to_string(),
            }),
        )
    }
}

/// Type information without an associated value, used for type inferencing and definition.
// TODO use idents instead of Strings when we have arena spans
#[derive(Derivative)]
#[derivative(Debug, Clone)]
pub enum TypeInfo {
    Unknown,
    UnknownGeneric {
        name: Ident,
    },
    Str(u64),
    UnsignedInteger(IntegerBits),
    Enum {
        name: Ident,
        type_parameters: Vec<TypeParameter>,
        variant_types: Vec<TypedEnumVariant>,
    },
    Struct {
        name: Ident,
        type_parameters: Vec<TypeParameter>,
        fields: Vec<TypedStructField>,
    },
    Boolean,
    /// For the type inference engine to use when a type references another type
    Ref(TypeId, Span),

    Tuple(Vec<TypeArgument>),
    /// Represents a type which contains methods to issue a contract call.
    /// The specific contract is identified via the `Ident` within.
    ContractCaller {
        abi_name: AbiName,
        // boxed for size
        address: Option<Box<TypedExpression>>,
    },
    /// A custom type could be a struct or similar if the name is in scope,
    /// or just a generic parameter if it is not.
    /// At parse time, there is no sense of scope, so this determination is not made
    /// until the semantic analysis stage.
    Custom {
        name: Ident,
        type_arguments: Vec<TypeArgument>,
    },
    SelfType,
    Byte,
    B256,
    /// This means that specific type of a number is not yet known. It will be
    /// determined via inference at a later time.
    Numeric,
    Contract,
    // used for recovering from errors in the ast
    ErrorRecovery,
    // Static, constant size arrays.
    Array(TypeId, usize),
    /// Represents the entire storage declaration struct
    /// Stored without initializers here, as typed struct fields,
    /// so type checking is able to treat it as a struct with fields.
    Storage {
        fields: Vec<TypedStructField>,
    },
}

// NOTE: Hash and PartialEq must uphold the invariant:
// k1 == k2 -> hash(k1) == hash(k2)
// https://doc.rust-lang.org/std/collections/struct.HashMap.html
impl Hash for TypeInfo {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            TypeInfo::Str(len) => {
                state.write_u8(1);
                len.hash(state);
            }
            TypeInfo::UnsignedInteger(bits) => {
                state.write_u8(2);
                bits.hash(state);
            }
            TypeInfo::Numeric => {
                state.write_u8(3);
            }
            TypeInfo::Boolean => {
                state.write_u8(4);
            }
            TypeInfo::Tuple(fields) => {
                state.write_u8(5);
                fields.hash(state);
            }
            TypeInfo::Byte => {
                state.write_u8(6);
            }
            TypeInfo::B256 => {
                state.write_u8(7);
            }
            TypeInfo::Enum {
                name,
                variant_types,
                type_parameters,
            } => {
                state.write_u8(8);
                name.hash(state);
                variant_types.hash(state);
                type_parameters.hash(state);
            }
            TypeInfo::Struct {
                name,
                fields,
                type_parameters,
            } => {
                state.write_u8(9);
                name.hash(state);
                fields.hash(state);
                type_parameters.hash(state);
            }
            TypeInfo::ContractCaller { abi_name, address } => {
                state.write_u8(10);
                abi_name.hash(state);
                let address = address
                    .as_ref()
                    .map(|x| x.span.as_str().to_string())
                    .unwrap_or_default();
                address.hash(state);
            }
            TypeInfo::Contract => {
                state.write_u8(11);
            }
            TypeInfo::ErrorRecovery => {
                state.write_u8(12);
            }
            TypeInfo::Unknown => {
                state.write_u8(13);
            }
            TypeInfo::SelfType => {
                state.write_u8(14);
            }
            TypeInfo::UnknownGeneric { name } => {
                state.write_u8(15);
                name.hash(state);
            }
            TypeInfo::Custom {
                name,
                type_arguments,
            } => {
                state.write_u8(16);
                name.hash(state);
                type_arguments.hash(state);
            }
            TypeInfo::Ref(id, _sp) => {
                state.write_u8(17);
                look_up_type_id(*id).hash(state);
            }
            TypeInfo::Array(elem_ty, count) => {
                state.write_u8(18);
                look_up_type_id(*elem_ty).hash(state);
                count.hash(state);
            }
            TypeInfo::Storage { fields } => {
                state.write_u8(19);
                fields.hash(state);
            }
        }
    }
}

// NOTE: Hash and PartialEq must uphold the invariant:
// k1 == k2 -> hash(k1) == hash(k2)
// https://doc.rust-lang.org/std/collections/struct.HashMap.html
impl PartialEq for TypeInfo {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Unknown, Self::Unknown) => true,
            (Self::Boolean, Self::Boolean) => true,
            (Self::SelfType, Self::SelfType) => true,
            (Self::Byte, Self::Byte) => true,
            (Self::B256, Self::B256) => true,
            (Self::Numeric, Self::Numeric) => true,
            (Self::Contract, Self::Contract) => true,
            (Self::ErrorRecovery, Self::ErrorRecovery) => true,
            (Self::UnknownGeneric { name: l }, Self::UnknownGeneric { name: r }) => l == r,
            (
                Self::Custom {
                    name: l_name,
                    type_arguments: l_type_args,
                },
                Self::Custom {
                    name: r_name,
                    type_arguments: r_type_args,
                },
            ) => l_name == r_name && l_type_args == r_type_args,
            (Self::Str(l), Self::Str(r)) => l == r,
            (Self::UnsignedInteger(l), Self::UnsignedInteger(r)) => l == r,
            (
                Self::Enum {
                    name: l_name,
                    variant_types: l_variant_types,
                    type_parameters: l_type_parameters,
                },
                Self::Enum {
                    name: r_name,
                    variant_types: r_variant_types,
                    type_parameters: r_type_parameters,
                },
            ) => {
                l_name == r_name
                    && l_variant_types == r_variant_types
                    && l_type_parameters == r_type_parameters
            }
            (
                Self::Struct {
                    name: l_name,
                    fields: l_fields,
                    type_parameters: l_type_parameters,
                },
                Self::Struct {
                    name: r_name,
                    fields: r_fields,
                    type_parameters: r_type_parameters,
                },
            ) => l_name == r_name && l_fields == r_fields && l_type_parameters == r_type_parameters,
            (Self::Ref(l, _sp1), Self::Ref(r, _sp2)) => look_up_type_id(*l) == look_up_type_id(*r),
            (Self::Tuple(l), Self::Tuple(r)) => l
                .iter()
                .zip(r.iter())
                .map(|(l, r)| look_up_type_id(l.type_id) == look_up_type_id(r.type_id))
                .all(|x| x),
            (
                Self::ContractCaller {
                    abi_name: l_abi_name,
                    address: l_address,
                },
                Self::ContractCaller {
                    abi_name: r_abi_name,
                    address: r_address,
                },
            ) => l_abi_name == r_abi_name && l_address == r_address,
            (Self::Array(l0, l1), Self::Array(r0, r1)) => {
                look_up_type_id(*l0) == look_up_type_id(*r0) && l1 == r1
            }
            (TypeInfo::Storage { fields: l_fields }, TypeInfo::Storage { fields: r_fields }) => {
                l_fields == r_fields
            }
            _ => false,
        }
    }
}

impl Eq for TypeInfo {}

impl Default for TypeInfo {
    fn default() -> Self {
        TypeInfo::Unknown
    }
}

impl fmt::Display for TypeInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use TypeInfo::*;
        let s = match self {
            Unknown => "unknown".into(),
            UnknownGeneric { name, .. } => name.to_string(),
            Str(x) => format!("str[{}]", x),
            UnsignedInteger(x) => match x {
                IntegerBits::Eight => "u8",
                IntegerBits::Sixteen => "u16",
                IntegerBits::ThirtyTwo => "u32",
                IntegerBits::SixtyFour => "u64",
            }
            .into(),
            Boolean => "bool".into(),
            Custom { name, .. } => format!("unresolved {}", name.as_str()),
            Ref(id, _sp) => format!("T{} ({})", id, (*id)),
            Tuple(fields) => {
                let field_strs = fields
                    .iter()
                    .map(|field| field.to_string())
                    .collect::<Vec<String>>();
                format!("({})", field_strs.join(", "))
            }
            SelfType => "Self".into(),
            Byte => "byte".into(),
            B256 => "b256".into(),
            Numeric => "numeric".into(),
            Contract => "contract".into(),
            ErrorRecovery => "unknown due to error".into(),
            Enum {
                name,
                type_parameters,
                ..
            } => print_inner_types(
                name.as_str().to_string(),
                type_parameters.iter().map(|x| x.type_id),
            ),
            Struct {
                name,
                type_parameters,
                ..
            } => print_inner_types(
                name.as_str().to_string(),
                type_parameters.iter().map(|x| x.type_id),
            ),
            ContractCaller { abi_name, .. } => {
                format!("contract caller {}", abi_name)
            }
            Array(elem_ty, count) => format!("[{}; {}]", elem_ty, count),
            Storage { .. } => "contract storage".into(),
        };
        write!(f, "{}", s)
    }
}

impl JsonAbiString for TypeInfo {
    fn json_abi_str(&self) -> String {
        use TypeInfo::*;
        match self {
            Unknown => "unknown".into(),
            UnknownGeneric { name, .. } => name.to_string(),
            Str(x) => format!("str[{}]", x),
            UnsignedInteger(x) => match x {
                IntegerBits::Eight => "u8",
                IntegerBits::Sixteen => "u16",
                IntegerBits::ThirtyTwo => "u32",
                IntegerBits::SixtyFour => "u64",
            }
            .into(),
            Boolean => "bool".into(),
            Custom { name, .. } => format!("unresolved {}", name.as_str()),
            Ref(id, _sp) => format!("T{} ({})", id, (*id).json_abi_str()),
            Tuple(fields) => {
                let field_strs = fields
                    .iter()
                    .map(|field| field.json_abi_str())
                    .collect::<Vec<String>>();
                format!("({})", field_strs.join(", "))
            }
            SelfType => "Self".into(),
            Byte => "byte".into(),
            B256 => "b256".into(),
            Numeric => "numeric".into(),
            Contract => "contract".into(),
            ErrorRecovery => "unknown due to error".into(),
            Enum { name, .. } => {
                format!("enum {}", name)
            }
            Struct { name, .. } => {
                format!("struct {}", name)
            }
            ContractCaller { abi_name, .. } => {
                format!("contract caller {}", abi_name)
            }
            Array(elem_ty, count) => format!("[{}; {}]", elem_ty.json_abi_str(), count),
            Storage { .. } => "contract storage".into(),
        }
    }
}

impl TypeInfo {
    /// maps a type to a name that is used when constructing function selectors
    pub(crate) fn to_selector_name(&self, error_msg_span: &Span) -> CompileResult<String> {
        use TypeInfo::*;
        let name = match self {
            Str(len) => format!("str[{}]", len),
            UnsignedInteger(bits) => {
                use IntegerBits::*;
                match bits {
                    Eight => "u8",
                    Sixteen => "u16",
                    ThirtyTwo => "u32",
                    SixtyFour => "u64",
                }
                .into()
            }
            Boolean => "bool".into(),

            Tuple(fields) => {
                let field_names = {
                    let names = fields
                        .iter()
                        .map(|field_type| {
                            resolve_type(field_type.type_id, error_msg_span)
                                .expect("unreachable?")
                                .to_selector_name(error_msg_span)
                        })
                        .collect::<Vec<CompileResult<String>>>();
                    let mut buf = vec![];
                    for name in names {
                        match name.value {
                            Some(value) => buf.push(value),
                            None => return name,
                        }
                    }
                    buf
                };

                format!("({})", field_names.join(","))
            }
            Byte => "byte".into(),
            B256 => "b256".into(),
            Struct { fields, .. } => {
                let names = fields
                    .iter()
                    .map(|field| {
                        resolve_type(field.r#type, error_msg_span)
                            .expect("unreachable?")
                            .to_selector_name(error_msg_span)
                    })
                    .collect::<Vec<CompileResult<String>>>();
                let mut buf = vec![];
                for name in names {
                    match name.value {
                        Some(value) => buf.push(value),
                        None => return name,
                    }
                }
                format!("s({})", buf.join(","))
            }
            Enum { variant_types, .. } => {
                let variant_names = {
                    let names = variant_types
                        .iter()
                        .map(|ty| {
                            let ty = match resolve_type(ty.r#type, error_msg_span) {
                                Err(e) => return err(vec![], vec![e.into()]),
                                Ok(ty) => ty,
                            };
                            ty.to_selector_name(error_msg_span)
                        })
                        .collect::<Vec<CompileResult<String>>>();
                    let mut buf = vec![];
                    for name in names {
                        match name.value {
                            Some(value) => buf.push(value),
                            None => return name,
                        }
                    }
                    buf
                };

                format!("e({})", variant_names.join(","))
            }
            Array(type_id, size) => {
                let name = look_up_type_id(*type_id).to_selector_name(error_msg_span);
                let name = match name.value {
                    Some(name) => name,
                    None => return name,
                };
                format!("a[{};{}]", name, size)
            }
            _ => {
                return err(
                    vec![],
                    vec![CompileError::InvalidAbiType {
                        span: error_msg_span.clone(),
                    }],
                )
            }
        };
        ok(name, vec![], vec![])
    }

    pub fn is_uninhabited(&self) -> bool {
        match self {
            TypeInfo::Enum { variant_types, .. } => variant_types
                .iter()
                .all(|variant_type| look_up_type_id(variant_type.r#type).is_uninhabited()),
            TypeInfo::Struct { fields, .. } => fields
                .iter()
                .any(|field| look_up_type_id(field.r#type).is_uninhabited()),
            TypeInfo::Tuple(fields) => fields
                .iter()
                .any(|field_type| look_up_type_id(field_type.type_id).is_uninhabited()),
            _ => false,
        }
    }

    pub fn is_zero_sized(&self) -> bool {
        match self {
            TypeInfo::Enum { variant_types, .. } => {
                let mut found_unit_variant = false;
                for variant_type in variant_types {
                    let type_info = look_up_type_id(variant_type.r#type);
                    if type_info.is_uninhabited() {
                        continue;
                    }
                    if type_info.is_zero_sized() && !found_unit_variant {
                        found_unit_variant = true;
                        continue;
                    }
                    return false;
                }
                true
            }
            TypeInfo::Struct { fields, .. } => {
                let mut all_zero_sized = true;
                for field in fields {
                    let type_info = look_up_type_id(field.r#type);
                    if type_info.is_uninhabited() {
                        return true;
                    }
                    if !type_info.is_zero_sized() {
                        all_zero_sized = false;
                    }
                }
                all_zero_sized
            }
            TypeInfo::Tuple(fields) => {
                let mut all_zero_sized = true;
                for field in fields {
                    let field_type = look_up_type_id(field.type_id);
                    if field_type.is_uninhabited() {
                        return true;
                    }
                    if !field_type.is_zero_sized() {
                        all_zero_sized = false;
                    }
                }
                all_zero_sized
            }
            _ => false,
        }
    }

    pub fn is_unit(&self) -> bool {
        match self {
            TypeInfo::Tuple(fields) => fields.is_empty(),
            _ => false,
        }
    }

    pub(crate) fn matches_type_parameter(&self, mapping: &TypeMapping) -> Option<TypeId> {
        use TypeInfo::*;
        match self {
            TypeInfo::Custom { .. } => {
                for (param, ty_id) in mapping.iter() {
                    if look_up_type_id(param.type_id) == *self {
                        return Some(*ty_id);
                    }
                }
                None
            }
            TypeInfo::UnknownGeneric { .. } => {
                for (param, ty_id) in mapping.iter() {
                    if look_up_type_id(param.type_id) == *self {
                        return Some(*ty_id);
                    }
                }
                None
            }
            TypeInfo::Struct {
                fields,
                name,
                type_parameters,
            } => {
                let mut new_fields = fields.clone();
                for new_field in new_fields.iter_mut() {
                    if let Some(matching_id) =
                        look_up_type_id(new_field.r#type).matches_type_parameter(mapping)
                    {
                        new_field.r#type =
                            insert_type(TypeInfo::Ref(matching_id, new_field.span.clone()));
                    }
                }
                let mut new_type_parameters = type_parameters.clone();
                for new_param in new_type_parameters.iter_mut() {
                    if let Some(matching_id) =
                        look_up_type_id(new_param.type_id).matches_type_parameter(mapping)
                    {
                        new_param.type_id =
                            insert_type(TypeInfo::Ref(matching_id, new_param.span().clone()));
                    }
                }
                Some(insert_type(TypeInfo::Struct {
                    fields: new_fields,
                    name: name.clone(),
                    type_parameters: new_type_parameters,
                }))
            }
            TypeInfo::Enum {
                variant_types,
                name,
                type_parameters,
            } => {
                let mut new_variants = variant_types.clone();
                for new_variant in new_variants.iter_mut() {
                    if let Some(matching_id) =
                        look_up_type_id(new_variant.r#type).matches_type_parameter(mapping)
                    {
                        new_variant.r#type =
                            insert_type(TypeInfo::Ref(matching_id, new_variant.span.clone()));
                    }
                }
                let mut new_type_parameters = type_parameters.clone();
                for new_param in new_type_parameters.iter_mut() {
                    if let Some(matching_id) =
                        look_up_type_id(new_param.type_id).matches_type_parameter(mapping)
                    {
                        new_param.type_id =
                            insert_type(TypeInfo::Ref(matching_id, new_param.span().clone()));
                    }
                }
                Some(insert_type(TypeInfo::Enum {
                    variant_types: new_variants,
                    type_parameters: new_type_parameters,
                    name: name.clone(),
                }))
            }
            TypeInfo::Array(ary_ty_id, count) => look_up_type_id(*ary_ty_id)
                .matches_type_parameter(mapping)
                .map(|matching_id| insert_type(TypeInfo::Array(matching_id, *count))),
            TypeInfo::Tuple(fields) => {
                let mut new_fields = Vec::new();
                let mut index = 0;
                while index < fields.len() {
                    let new_field_id_opt =
                        look_up_type_id(fields[index].type_id).matches_type_parameter(mapping);
                    if let Some(new_field_id) = new_field_id_opt {
                        new_fields.extend(fields[..index].iter().cloned());
                        new_fields.push(TypeArgument {
                            type_id: insert_type(TypeInfo::Ref(
                                new_field_id,
                                fields[index].span.clone(),
                            )),
                            span: fields[index].span.clone(),
                        });
                        index += 1;
                        break;
                    }
                    index += 1;
                }
                while index < fields.len() {
                    let new_field = match look_up_type_id(fields[index].type_id)
                        .matches_type_parameter(mapping)
                    {
                        Some(new_field_id) => TypeArgument {
                            type_id: insert_type(TypeInfo::Ref(
                                new_field_id,
                                fields[index].span.clone(),
                            )),
                            span: fields[index].span.clone(),
                        },
                        None => fields[index].clone(),
                    };
                    new_fields.push(new_field);
                    index += 1;
                }
                if new_fields.is_empty() {
                    None
                } else {
                    Some(insert_type(TypeInfo::Tuple(new_fields)))
                }
            }
            Unknown
            | Str(..)
            | UnsignedInteger(..)
            | Boolean
            | Ref(..)
            | ContractCaller { .. }
            | SelfType
            | Byte
            | B256
            | Numeric
            | Contract
            | Storage { .. }
            | ErrorRecovery => None,
        }
    }

    /// Given a `TypeInfo` `self`, check to see if `self` is currently
    /// supported in match expressions, and return an error if it is not.
    pub(crate) fn expect_is_supported_in_match_expressions(
        &self,
        span: &Span,
    ) -> CompileResult<()> {
        let warnings = vec![];
        let mut errors = vec![];
        match self {
            TypeInfo::Ref(type_id, _) => {
                look_up_type_id(*type_id).expect_is_supported_in_match_expressions(span)
            }
            TypeInfo::UnsignedInteger(_)
            | TypeInfo::Enum { .. }
            | TypeInfo::Struct { .. }
            | TypeInfo::Boolean
            | TypeInfo::Tuple(_)
            | TypeInfo::Byte
            | TypeInfo::B256
            | TypeInfo::UnknownGeneric { .. }
            | TypeInfo::Numeric => ok((), warnings, errors),
            TypeInfo::Unknown
            | TypeInfo::ContractCaller { .. }
            | TypeInfo::Custom { .. }
            | TypeInfo::SelfType
            | TypeInfo::Str(_)
            | TypeInfo::Contract
            | TypeInfo::ErrorRecovery
            | TypeInfo::Array(_, _)
            | TypeInfo::Storage { .. } => {
                errors.push(CompileError::Unimplemented(
                    "matching on this type is unsupported right now",
                    span.clone(),
                ));
                err(warnings, errors)
            }
        }
    }

    /// Given a `TypeInfo` `self`, analyze `self` and return all nested
    /// `TypeInfo`'s found in `self`, including `self`.
    pub(crate) fn extract_nested_types(self, span: &Span) -> CompileResult<Vec<TypeInfo>> {
        let mut warnings = vec![];
        let mut errors = vec![];
        let mut all_nested_types = vec![self.clone()];
        match self {
            TypeInfo::Enum { variant_types, .. } => {
                for variant_type in variant_types.iter() {
                    let mut nested_types = check!(
                        look_up_type_id(variant_type.r#type).extract_nested_types(span),
                        return err(warnings, errors),
                        warnings,
                        errors
                    );
                    all_nested_types.append(&mut nested_types);
                }
            }
            TypeInfo::Struct { fields, .. } => {
                for field in fields.iter() {
                    let mut nested_types = check!(
                        look_up_type_id(field.r#type).extract_nested_types(span),
                        return err(warnings, errors),
                        warnings,
                        errors
                    );
                    all_nested_types.append(&mut nested_types);
                }
            }
            TypeInfo::Ref(type_id, _) => {
                let mut nested_types = check!(
                    look_up_type_id(type_id).extract_nested_types(span),
                    return err(warnings, errors),
                    warnings,
                    errors
                );
                all_nested_types.append(&mut nested_types);
            }
            TypeInfo::Tuple(type_arguments) => {
                for type_argument in type_arguments.iter() {
                    let mut nested_types = check!(
                        look_up_type_id(type_argument.type_id).extract_nested_types(span),
                        return err(warnings, errors),
                        warnings,
                        errors
                    );
                    all_nested_types.append(&mut nested_types);
                }
            }
            TypeInfo::Array(type_id, _) => {
                let mut nested_types = check!(
                    look_up_type_id(type_id).extract_nested_types(span),
                    return err(warnings, errors),
                    warnings,
                    errors
                );
                all_nested_types.append(&mut nested_types);
            }
            TypeInfo::Storage { fields } => {
                for field in fields.iter() {
                    let mut nested_types = check!(
                        look_up_type_id(field.r#type).extract_nested_types(span),
                        return err(warnings, errors),
                        warnings,
                        errors
                    );
                    all_nested_types.append(&mut nested_types);
                }
            }
            TypeInfo::Unknown
            | TypeInfo::UnknownGeneric { .. }
            | TypeInfo::Str(_)
            | TypeInfo::UnsignedInteger(_)
            | TypeInfo::Boolean
            | TypeInfo::ContractCaller { .. }
            | TypeInfo::Byte
            | TypeInfo::B256
            | TypeInfo::Numeric
            | TypeInfo::Contract
            | TypeInfo::ErrorRecovery => {}
            TypeInfo::Custom { .. } | TypeInfo::SelfType => {
                errors.push(CompileError::Internal(
                    "did not expect to find this type here",
                    span.clone(),
                ));
                return err(warnings, errors);
            }
        }
        ok(all_nested_types, warnings, errors)
    }

    /// Given a `TypeInfo` `self` and a list of `Ident`'s `subfields`,
    /// iterate through the elements of `subfields` as `subfield`,
    /// and recursively apply `subfield` to `self`.
    ///
    /// Returns a `TypedStructField` when all `subfields` could be
    /// applied without error.
    ///
    /// Returns an error when subfields could not be applied:
    /// 1) in the case where `self` is not a `TypeInfo::Struct`
    /// 2) in the case where `subfields` is empty
    /// 3) in the case where a `subfield` does not exist on `self`
    pub(crate) fn apply_subfields(
        &self,
        subfields: &[Ident],
        span: &Span,
    ) -> CompileResult<TypedStructField> {
        let mut warnings = vec![];
        let mut errors = vec![];
        match (self, subfields.split_first()) {
            (TypeInfo::Struct { .. }, None) => err(warnings, errors),
            (TypeInfo::Struct { name, fields, .. }, Some((first, rest))) => {
                let field = match fields
                    .iter()
                    .find(|field| field.name.as_str() == first.as_str())
                {
                    Some(field) => field.clone(),
                    None => {
                        // gather available fields for the error message
                        let available_fields =
                            fields.iter().map(|x| x.name.as_str()).collect::<Vec<_>>();
                        errors.push(CompileError::FieldNotFound {
                            field_name: first.clone(),
                            struct_name: name.clone(),
                            available_fields: available_fields.join(", "),
                        });
                        return err(warnings, errors);
                    }
                };
                let field = if rest.is_empty() {
                    field
                } else {
                    check!(
                        look_up_type_id(field.r#type).apply_subfields(rest, span),
                        return err(warnings, errors),
                        warnings,
                        errors
                    )
                };
                ok(field, warnings, errors)
            }
            (TypeInfo::ErrorRecovery, _) => {
                // dont create a new error in this case
                err(warnings, errors)
            }
            (type_info, _) => {
                errors.push(CompileError::FieldAccessOnNonStruct {
                    actually: type_info.to_string(),
                    span: span.clone(),
                });
                err(warnings, errors)
            }
        }
    }

    /// Given a `TypeInfo` `self`, expect that `self` is a `TypeInfo::Tuple`,
    /// and return its contents.
    ///
    /// Returns an error if `self` is not a `TypeInfo::Tuple`.
    pub(crate) fn expect_tuple(
        &self,
        debug_string: impl Into<String>,
        debug_span: &Span,
    ) -> CompileResult<&Vec<TypeArgument>> {
        let warnings = vec![];
        let errors = vec![];
        match self {
            TypeInfo::Tuple(elems) => ok(elems, warnings, errors),
            TypeInfo::ErrorRecovery => err(warnings, errors),
            a => err(
                vec![],
                vec![CompileError::NotATuple {
                    name: debug_string.into(),
                    span: debug_span.clone(),
                    actually: a.to_string(),
                }],
            ),
        }
    }

    /// Given a `TypeInfo` `self`, expect that `self` is a `TypeInfo::Enum`,
    /// and return its contents.
    ///
    /// Returns an error if `self` is not a `TypeInfo::Enum`.
    pub(crate) fn expect_enum(
        &self,
        debug_string: impl Into<String>,
        debug_span: &Span,
    ) -> CompileResult<(&Ident, &Vec<TypedEnumVariant>)> {
        let warnings = vec![];
        let errors = vec![];
        match self {
            TypeInfo::Enum {
                name,
                variant_types,
                ..
            } => ok((name, variant_types), warnings, errors),
            TypeInfo::ErrorRecovery => err(warnings, errors),
            a => err(
                vec![],
                vec![CompileError::NotAnEnum {
                    name: debug_string.into(),
                    span: debug_span.clone(),
                    actually: a.to_string(),
                }],
            ),
        }
    }
}

fn print_inner_types(name: String, inner_types: impl Iterator<Item = TypeId>) -> String {
    let inner_types = inner_types.map(|x| x.to_string()).collect::<Vec<_>>();
    format!(
        "{}{}",
        name,
        if inner_types.is_empty() {
            "".into()
        } else {
            format!("<{}>", inner_types.join(", "))
        }
    )
}