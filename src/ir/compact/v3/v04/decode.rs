// Copyright 2025-2026 STARGA Inc.
// Licensed under the Apache License, Version 2.0.

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::io::{Cursor, Read};

use crate::ir::compact::v2::zigzag_decode;
use crate::ir::compact::v3::emit::{
    OP_BINOP, OP_CALL, OP_CONST_F64, OP_CONST_I64, OP_FN_DEF, OP_OUTPUT, OP_PARAM, OP_RETURN,
    byte_to_binop,
};
use crate::ir::compact::v3::{
    MAX_MIC3_INPUT, MIC3_MAGIC, MIC3_VERSION_V04, Mic3Error, ParsedMic3Prefix,
};
use crate::ir::{IRModule, Instr, ValueId, verify_canonical_metadata};
use crate::types::{
    CanonicalModuleTypes, FieldDraft, FunctionDeclaration, FunctionIdentity, FunctionKind,
    FunctionSemanticTypes, ScalarType, SchemaDraft, SchemaIdentity, SchemaRegistryBuilder,
    SemanticType, TypeExpr,
};

use super::{
    DESCRIPTOR_CHARGE, FIELD_CHARGE, FUNCTION_CHARGE, FUNCTION_EXTERNAL, FUNCTION_INTRINSIC,
    FUNCTION_LOCAL, INSTRUCTION_CHARGE, KNOWN_SURFACE_BITS, MAX_INSTR_DEPTH, NAMED_VALUE_CHARGE,
    PARAMETER_CHARGE, REQUIRED_STD_SURFACE, SCALAR_BF16, SCALAR_BOOL, SCALAR_F16, SCALAR_F32,
    SCALAR_F64, SCALAR_I32, SCALAR_I64, SCALAR_Q16, SCALAR_U32, SCHEMA_CHARGE,
    STRING_REFERENCE_MULTIPLIER, STRING_SLOT_CHARGE, TYPE_DYNAMIC_ARRAY, TYPE_FIXED_ARRAY,
    TYPE_RECORD_REF, TYPE_SCALAR, VALUE_ID_CHARGE, VALUE_ROW_CHARGE, decoded_budget_limit,
    validate_core_value_ids,
};

const MAX_TYPE_DEPTH: usize = 64;

fn error(message: impl Into<String>) -> Mic3Error {
    Mic3Error {
        message: message.into(),
    }
}

struct DecodeBudget {
    remaining: usize,
}

impl DecodeBudget {
    fn for_input(input: usize) -> Self {
        Self {
            remaining: decoded_budget_limit(input),
        }
    }

    fn charge(&mut self, count: usize, unit: usize, label: &str) -> Result<(), Mic3Error> {
        let amount = count
            .checked_mul(unit)
            .ok_or_else(|| error(format!("v0x04 {label} allocation charge overflow")))?;
        self.remaining = self.remaining.checked_sub(amount).ok_or_else(|| {
            error(format!(
                "v0x04 decoded allocation budget exhausted while admitting {label}"
            ))
        })?;
        Ok(())
    }

    fn charge_bytes(&mut self, bytes: usize, label: &str) -> Result<(), Mic3Error> {
        self.charge(bytes, 1, label)
    }
}

struct Decoder<'a> {
    cursor: Cursor<&'a [u8]>,
    limit: usize,
    budget: DecodeBudget,
}

impl<'a> Decoder<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self {
            cursor: Cursor::new(data),
            limit: data.len(),
            budget: DecodeBudget::for_input(data.len()),
        }
    }

    fn position(&self) -> Result<usize, Mic3Error> {
        usize::try_from(self.cursor.position())
            .map_err(|_| error("v0x04 cursor position does not fit usize"))
    }

    fn remaining(&self) -> Result<usize, Mic3Error> {
        self.limit
            .checked_sub(self.position()?)
            .ok_or_else(|| error("v0x04 cursor moved beyond input"))
    }

    fn byte(&mut self) -> Result<u8, Mic3Error> {
        let mut byte = [0_u8; 1];
        self.cursor.read_exact(&mut byte)?;
        Ok(byte[0])
    }

    fn fixed<const N: usize>(&mut self) -> Result<[u8; N], Mic3Error> {
        let mut bytes = [0_u8; N];
        self.cursor.read_exact(&mut bytes)?;
        Ok(bytes)
    }

    fn uleb(&mut self) -> Result<u64, Mic3Error> {
        let mut value = 0_u64;
        for index in 0..10 {
            let byte = self.byte()?;
            if index == 9 && byte > 1 {
                return Err(error("v0x04 ULEB128 exceeds u64"));
            }
            value |= u64::from(byte & 0x7f) << (index * 7);
            if byte & 0x80 == 0 {
                if index != 0 && byte == 0 {
                    return Err(error("non-minimal v0x04 ULEB128"));
                }
                return Ok(value);
            }
        }
        Err(error("unterminated v0x04 ULEB128"))
    }

    fn usize(&mut self, label: &str) -> Result<usize, Mic3Error> {
        usize::try_from(self.uleb()?)
            .map_err(|_| error(format!("v0x04 {label} does not fit usize")))
    }

    fn count(&mut self, unit: usize, label: &str) -> Result<usize, Mic3Error> {
        let count = self.usize(label)?;
        let remaining = self.remaining()?;
        if count > remaining {
            return Err(error(format!(
                "v0x04 {label} count {count} exceeds {remaining} remaining bytes"
            )));
        }
        self.budget.charge(count, unit, label)?;
        Ok(count)
    }

    fn vid(&mut self) -> Result<ValueId, Mic3Error> {
        Ok(ValueId(self.usize("ValueId")?))
    }

    fn opt_vid(&mut self) -> Result<Option<ValueId>, Mic3Error> {
        match self.byte()? {
            0 => Ok(None),
            1 => Ok(Some(self.vid()?)),
            tag => Err(error(format!("invalid v0x04 optional ValueId tag {tag}"))),
        }
    }

    fn opt_f64(&mut self) -> Result<Option<f64>, Mic3Error> {
        match self.byte()? {
            0 => Ok(None),
            1 => Ok(Some(f64::from_bits(u64::from_le_bytes(self.fixed()?)))),
            tag => Err(error(format!("invalid v0x04 optional f64 tag {tag}"))),
        }
    }

    fn string_ref(&mut self, strings: &[String]) -> Result<String, Mic3Error> {
        let index = self.usize("string index")?;
        let string = strings.get(index).ok_or_else(|| {
            error(format!(
                "v0x04 string index {index} out of bounds for {} entries",
                strings.len()
            ))
        })?;
        let charge = string
            .len()
            .checked_mul(STRING_REFERENCE_MULTIPLIER)
            .ok_or_else(|| error("v0x04 string-reference charge overflow"))?;
        self.budget
            .charge_bytes(charge, "owned string references and projections")?;
        Ok(string.clone())
    }

    fn schema_ref(&mut self, identities: &[SchemaIdentity]) -> Result<SchemaIdentity, Mic3Error> {
        let index = self.usize("schema index")?;
        let identity = identities.get(index).ok_or_else(|| {
            error(format!(
                "v0x04 schema index {index} out of bounds for {} entries",
                identities.len()
            ))
        })?;
        let bytes = identity
            .owner()
            .len()
            .checked_add(identity.name().len())
            .and_then(|value| value.checked_mul(STRING_REFERENCE_MULTIPLIER))
            .ok_or_else(|| error("v0x04 schema identity clone charge overflow"))?;
        self.budget
            .charge_bytes(bytes, "schema identity projection")?;
        Ok(identity.clone())
    }

    fn function_ref(
        &mut self,
        identities: &[FunctionIdentity],
    ) -> Result<FunctionIdentity, Mic3Error> {
        let index = self.usize("function index")?;
        let identity = identities.get(index).ok_or_else(|| {
            error(format!(
                "v0x04 function index {index} out of bounds for {} entries",
                identities.len()
            ))
        })?;
        let bytes = identity
            .owner()
            .len()
            .checked_add(identity.name().len())
            .and_then(|value| value.checked_mul(STRING_REFERENCE_MULTIPLIER))
            .ok_or_else(|| error("v0x04 function identity clone charge overflow"))?;
        self.budget
            .charge_bytes(bytes, "function identity projection")?;
        Ok(identity.clone())
    }

    fn read_strings(&mut self) -> Result<Vec<String>, Mic3Error> {
        let count = self.count(STRING_SLOT_CHARGE, "string table")?;
        let mut strings = Vec::with_capacity(count);
        for _ in 0..count {
            let length = self.usize("string length")?;
            let remaining = self.remaining()?;
            if length > remaining {
                return Err(error(format!(
                    "v0x04 string length {length} exceeds {remaining} remaining bytes"
                )));
            }
            self.budget.charge_bytes(length, "string bytes")?;
            let mut bytes = vec![0_u8; length];
            self.cursor.read_exact(&mut bytes)?;
            let string = String::from_utf8(bytes)
                .map_err(|_| error("invalid UTF-8 in v0x04 string table"))?;
            if strings
                .last()
                .is_some_and(|previous: &String| previous.as_bytes() >= string.as_bytes())
            {
                return Err(error(
                    "v0x04 string table must be unique and sorted by UTF-8 bytes",
                ));
            }
            strings.push(string);
        }
        Ok(strings)
    }

    fn read_type_expr(
        &mut self,
        schemas: &[SchemaIdentity],
        depth: usize,
    ) -> Result<TypeExpr, Mic3Error> {
        if depth > MAX_TYPE_DEPTH {
            return Err(error("v0x04 semantic descriptor exceeds depth 64"));
        }
        self.budget
            .charge(1, DESCRIPTOR_CHARGE, "semantic descriptor")?;
        match self.byte()? {
            TYPE_SCALAR => Ok(TypeExpr::Scalar(read_scalar(self.byte()?)?)),
            TYPE_RECORD_REF => Ok(TypeExpr::RecordRef(self.schema_ref(schemas)?)),
            TYPE_FIXED_ARRAY => {
                let extent = self.uleb()?;
                if extent > u64::from(u32::MAX) {
                    return Err(error("v0x04 fixed-array extent exceeds u32::MAX"));
                }
                Ok(TypeExpr::FixedArray {
                    extent,
                    element: Box::new(self.read_type_expr(schemas, depth + 1)?),
                })
            }
            TYPE_DYNAMIC_ARRAY => Ok(TypeExpr::DynamicArray {
                element: Box::new(self.read_type_expr(schemas, depth + 1)?),
            }),
            tag => Err(error(format!("unknown v0x04 semantic type tag {tag}"))),
        }
    }

    fn read_semantic_type(
        &mut self,
        schemas: &[SchemaIdentity],
        depth: usize,
    ) -> Result<SemanticType, Mic3Error> {
        if depth > MAX_TYPE_DEPTH {
            return Err(error("v0x04 semantic descriptor exceeds depth 64"));
        }
        self.budget
            .charge(1, DESCRIPTOR_CHARGE, "semantic descriptor")?;
        match self.byte()? {
            TYPE_SCALAR => Ok(SemanticType::Scalar(read_scalar(self.byte()?)?)),
            TYPE_RECORD_REF => Ok(SemanticType::RecordRef(self.schema_ref(schemas)?)),
            TYPE_FIXED_ARRAY => {
                let extent = self.uleb()?;
                if extent > u64::from(u32::MAX) {
                    return Err(error("v0x04 fixed-array extent exceeds u32::MAX"));
                }
                Ok(SemanticType::FixedArray {
                    extent,
                    element: Box::new(self.read_semantic_type(schemas, depth + 1)?),
                })
            }
            TYPE_DYNAMIC_ARRAY => Ok(SemanticType::DynamicArray {
                element: Box::new(self.read_semantic_type(schemas, depth + 1)?),
            }),
            tag => Err(error(format!("unknown v0x04 semantic type tag {tag}"))),
        }
    }

    fn require_zero_surface_count(&mut self, label: &str) -> Result<(), Mic3Error> {
        let count = self.usize(label)?;
        if count > self.remaining()? {
            return Err(error(format!(
                "v0x04 {label} count exceeds remaining encoded bytes"
            )));
        }
        if count != 0 {
            return Err(error(format!(
                "v0x04 required_surface_bits mismatch: unsupported {label} is populated"
            )));
        }
        Ok(())
    }

    fn read_instr(
        &mut self,
        strings: &[String],
        schemas: &[SchemaIdentity],
        functions: &[FunctionIdentity],
        depth: usize,
    ) -> Result<Instr, Mic3Error> {
        if depth >= MAX_INSTR_DEPTH {
            return Err(error("v0x04 instruction nesting exceeds depth 256"));
        }
        self.budget.charge(1, INSTRUCTION_CHARGE, "instruction")?;
        let opcode = self.byte()?;
        match opcode {
            OP_CONST_I64 => Ok(Instr::ConstI64(self.vid()?, zigzag_decode(self.uleb()?))),
            OP_CONST_F64 => Ok(Instr::ConstF64(
                self.vid()?,
                f64::from_bits(u64::from_le_bytes(self.fixed()?)),
            )),
            OP_BINOP => {
                let dst = self.vid()?;
                let tag = self.byte()?;
                if tag > 0x0a {
                    return Err(error(
                        "v0x04 required_surface_bits mismatch: unsupported BinOp tag",
                    ));
                }
                let op = byte_to_binop(tag)
                    .ok_or_else(|| error(format!("unknown v0x04 BinOp tag {tag}")))?;
                Ok(Instr::BinOp {
                    dst,
                    op,
                    lhs: self.vid()?,
                    rhs: self.vid()?,
                })
            }
            OP_OUTPUT => Ok(Instr::Output(self.vid()?)),
            OP_FN_DEF => {
                let name = self.string_ref(strings)?;
                let parameter_count = self.count(NAMED_VALUE_CHARGE, "function parameters")?;
                let mut parameters = Vec::with_capacity(parameter_count);
                for _ in 0..parameter_count {
                    parameters.push((self.string_ref(strings)?, self.vid()?));
                }
                let ret_id = self.opt_vid()?;
                let reap_threshold = self.opt_f64()?;
                let body_count = self.count(INSTRUCTION_CHARGE, "function body")?;
                let mut body = Vec::with_capacity(body_count);
                for _ in 0..body_count {
                    body.push(self.read_instr(strings, schemas, functions, depth + 1)?);
                }
                self.require_zero_surface_count("function legacy ArrayType table")?;
                let identity = self.function_ref(functions)?;
                let row_count = self.count(VALUE_ROW_CHARGE, "function semantic values")?;
                let mut semantic = FunctionSemanticTypes::new(identity);
                let mut previous = None;
                for _ in 0..row_count {
                    let value = self.vid()?;
                    require_increasing_value(previous, value, "function semantic values")?;
                    previous = Some(value);
                    let semantic_type = self.read_semantic_type(schemas, 0)?;
                    semantic
                        .set_value_type(value, semantic_type)
                        .map_err(|cause| error(format!("invalid v0x04 function table: {cause}")))?;
                }
                Ok(Instr::FnDef {
                    name,
                    params: parameters,
                    ret_id,
                    body,
                    reap_threshold,
                    semantic_types: Some(Box::new(semantic)),
                    #[cfg(feature = "std-surface")]
                    value_types: std::collections::BTreeMap::new(),
                })
            }
            OP_CALL => {
                let dst = self.vid()?;
                let name = self.string_ref(strings)?;
                let argument_count = self.count(VALUE_ID_CHARGE, "call arguments")?;
                let mut arguments = Vec::with_capacity(argument_count);
                for _ in 0..argument_count {
                    arguments.push(self.vid()?);
                }
                Ok(Instr::Call {
                    dst,
                    name,
                    args: arguments,
                    resolved_callee: Some(Box::new(self.function_ref(functions)?)),
                })
            }
            OP_RETURN => Ok(Instr::Return {
                value: self.opt_vid()?,
            }),
            OP_PARAM => Ok(Instr::Param {
                dst: self.vid()?,
                name: self.string_ref(strings)?,
                index: self.usize("parameter index")?,
            }),
            0x19..=0x25 | 0x28 | 0x29 | 0x2b => Err(error(format!(
                "v0x04 required_surface_bits mismatch: opcode 0x{opcode:02x} needs std-surface"
            ))),
            _ => Err(error(format!(
                "opcode 0x{opcode:02x} is outside the v0x04 core scalar subset"
            ))),
        }
    }
}

pub(crate) fn parse_v04_prefix(data: &[u8]) -> Result<ParsedMic3Prefix, Mic3Error> {
    if data.len() > MAX_MIC3_INPUT {
        return Err(error(format!(
            "mic@3 input too large: {} bytes (max {MAX_MIC3_INPUT})",
            data.len()
        )));
    }
    let mut input = Decoder::new(data);
    if input.fixed::<4>()? != MIC3_MAGIC {
        return Err(error("invalid MIC3 magic for v0x04"));
    }
    if input.byte()? != MIC3_VERSION_V04 {
        return Err(error("v0x04 decoder received another version"));
    }
    let surface = input.uleb()?;
    if surface & !KNOWN_SURFACE_BITS != 0 {
        return Err(error(format!(
            "unknown v0x04 required_surface_bits 0x{surface:x}"
        )));
    }
    if surface & REQUIRED_STD_SURFACE != 0 {
        return Err(error(
            "v0x04 required std-surface content is unsupported by the core codec",
        ));
    }
    let strings = input.read_strings()?;

    let schema_count = input.count(SCHEMA_CHARGE, "schema headers")?;
    let mut schema_identities = Vec::with_capacity(schema_count);
    for _ in 0..schema_count {
        let identity =
            SchemaIdentity::new(input.string_ref(&strings)?, input.string_ref(&strings)?);
        if schema_identities
            .last()
            .is_some_and(|previous| compare_schema_identity(previous, &identity) != Ordering::Less)
        {
            return Err(error("v0x04 schema identities are not strictly sorted"));
        }
        schema_identities.push(identity);
    }
    let mut builder = SchemaRegistryBuilder::default();
    for identity in &schema_identities {
        let field_count = input.count(FIELD_CHARGE, "schema fields")?;
        let mut fields = Vec::with_capacity(field_count);
        for _ in 0..field_count {
            fields.push(FieldDraft::new(
                input.string_ref(&strings)?,
                input.read_type_expr(&schema_identities, 0)?,
            ));
        }
        builder
            .add_schema(SchemaDraft::new(identity.clone(), fields))
            .map_err(|cause| error(format!("invalid v0x04 schema: {cause}")))?;
    }
    let registry = builder
        .finish()
        .map_err(|cause| error(format!("invalid v0x04 registry: {cause}")))?;

    let function_count = input.count(FUNCTION_CHARGE, "function declarations")?;
    let mut function_identities = Vec::with_capacity(function_count);
    let mut function_declarations = BTreeMap::new();
    for _ in 0..function_count {
        let identity =
            FunctionIdentity::new(input.string_ref(&strings)?, input.string_ref(&strings)?);
        if function_identities.last().is_some_and(|previous| {
            compare_function_identity(previous, &identity) != Ordering::Less
        }) {
            return Err(error("v0x04 function identities are not strictly sorted"));
        }
        let kind = match input.byte()? {
            FUNCTION_LOCAL => FunctionKind::Local,
            FUNCTION_EXTERNAL => FunctionKind::External,
            FUNCTION_INTRINSIC => FunctionKind::Intrinsic,
            tag => return Err(error(format!("unknown v0x04 function kind {tag}"))),
        };
        let parameter_count = input.count(PARAMETER_CHARGE, "signature parameters")?;
        let mut parameters = Vec::with_capacity(parameter_count);
        for _ in 0..parameter_count {
            parameters.push(input.read_semantic_type(&schema_identities, 0)?);
        }
        let return_type = match input.byte()? {
            0 => None,
            1 => Some(input.read_semantic_type(&schema_identities, 0)?),
            tag => return Err(error(format!("invalid v0x04 return-present tag {tag}"))),
        };
        let declaration = FunctionDeclaration::new(
            identity.clone(),
            kind,
            crate::types::FunctionSignature::new(parameters, return_type),
        );
        if function_declarations
            .insert(identity.clone(), declaration)
            .is_some()
        {
            return Err(error("duplicate v0x04 function declaration"));
        }
        function_identities.push(identity);
    }

    let next_id = input.usize("next_id")?;
    let export_count = input.count(STRING_SLOT_CHARGE, "exports")?;
    let mut exports = std::collections::HashSet::with_capacity(export_count);
    let mut previous_export = None;
    for _ in 0..export_count {
        let index = input.usize("export string index")?;
        if previous_export.is_some_and(|previous| previous >= index) {
            return Err(error("v0x04 export references are not strictly sorted"));
        }
        previous_export = Some(index);
        let value = strings
            .get(index)
            .ok_or_else(|| error("v0x04 export string index is out of bounds"))?;
        let charge = value
            .len()
            .checked_mul(STRING_REFERENCE_MULTIPLIER)
            .ok_or_else(|| error("v0x04 export string projection charge overflow"))?;
        input
            .budget
            .charge_bytes(charge, "export string projection")?;
        exports.insert(value.clone());
    }
    let instruction_count = input.count(INSTRUCTION_CHARGE, "module instructions")?;
    let mut instructions = Vec::with_capacity(instruction_count);
    for _ in 0..instruction_count {
        instructions.push(input.read_instr(
            &strings,
            &schema_identities,
            &function_identities,
            0,
        )?);
    }

    for label in [
        "struct_defs",
        "const_array_defs",
        "repr_c_structs",
        "module legacy ArrayType table",
    ] {
        input.require_zero_surface_count(label)?;
    }
    let row_count = input.count(VALUE_ROW_CHARGE, "module semantic values")?;
    let mut module_values = BTreeMap::new();
    let mut previous = None;
    for _ in 0..row_count {
        let value = input.vid()?;
        require_increasing_value(previous, value, "module semantic values")?;
        previous = Some(value);
        let semantic_type = input.read_semantic_type(&schema_identities, 0)?;
        if module_values.insert(value, semantic_type).is_some() {
            return Err(error("duplicate v0x04 module semantic value"));
        }
    }

    let bundle =
        CanonicalModuleTypes::from_parts_checked(registry, module_values, function_declarations)
            .map_err(|cause| error(format!("invalid v0x04 canonical parts: {cause}")))?;

    let mut module = IRModule::new();
    module.next_id = next_id;
    module.exports = exports;
    module.instrs = instructions;
    module.canonical_types = Some(Box::new(bundle));
    validate_core_value_ids(&module).map_err(error)?;
    verify_canonical_metadata(&module)
        .map_err(|cause| error(format!("invalid v0x04 canonical metadata: {cause}")))?;
    let has_authority = module
        .canonical_types
        .as_ref()
        .is_some_and(|bundle| !bundle.is_empty())
        || crate::ir::canonical_verify::instruction_metadata_present(&module.instrs);
    if !has_authority {
        return Err(error("v0x04 body carries no semantic authority"));
    }

    let consumed = input.position()?;
    input
        .budget
        .charge_bytes(consumed, "canonical re-emission")?;
    let canonical = super::emit_v04(&module)
        .map_err(|cause| error(format!("cannot canonically re-emit v0x04 body: {cause}")))?;
    if canonical.as_slice() != &data[..consumed] {
        return Err(error("noncanonical v0x04 body encoding"));
    }
    Ok(ParsedMic3Prefix { module, consumed })
}

fn read_scalar(tag: u8) -> Result<ScalarType, Mic3Error> {
    match tag {
        SCALAR_I32 => Ok(ScalarType::I32),
        SCALAR_I64 => Ok(ScalarType::I64),
        SCALAR_U32 => Ok(ScalarType::U32),
        SCALAR_F32 => Ok(ScalarType::F32),
        SCALAR_F64 => Ok(ScalarType::F64),
        SCALAR_BOOL => Ok(ScalarType::Bool),
        SCALAR_BF16 => Ok(ScalarType::BF16),
        SCALAR_F16 => Ok(ScalarType::F16),
        SCALAR_Q16 => Ok(ScalarType::Q16),
        _ => Err(error(format!("unknown v0x04 scalar tag {tag}"))),
    }
}

fn compare_schema_identity(left: &SchemaIdentity, right: &SchemaIdentity) -> Ordering {
    left.owner()
        .as_bytes()
        .cmp(right.owner().as_bytes())
        .then_with(|| left.name().as_bytes().cmp(right.name().as_bytes()))
}

fn compare_function_identity(left: &FunctionIdentity, right: &FunctionIdentity) -> Ordering {
    left.owner()
        .as_bytes()
        .cmp(right.owner().as_bytes())
        .then_with(|| left.name().as_bytes().cmp(right.name().as_bytes()))
}

fn require_increasing_value(
    previous: Option<ValueId>,
    current: ValueId,
    label: &str,
) -> Result<(), Mic3Error> {
    if previous.is_some_and(|previous| previous.0 >= current.0) {
        return Err(error(format!(
            "v0x04 {label} rows are not strictly sorted by ValueId"
        )));
    }
    Ok(())
}
