use super::*;

fn id(owner: &str, name: &str) -> SchemaIdentity {
    SchemaIdentity::new(owner, name)
}

fn record(identity: SchemaIdentity, fields: Vec<FieldDraft>) -> SchemaDraft {
    SchemaDraft::new(identity, fields)
}

fn scalar(field: &str, ty: ScalarType) -> FieldDraft {
    FieldDraft::new(field, TypeExpr::Scalar(ty))
}

#[test]
fn schema_ids_are_insertion_order_independent() {
    let a = record(id("pkg.core", "A"), vec![scalar("x", ScalarType::I64)]);
    let b = record(id("pkg.core", "B"), vec![scalar("y", ScalarType::F64)]);
    let mut first = SchemaRegistryBuilder::default();
    first.add_schema(a.clone()).unwrap();
    first.add_schema(b.clone()).unwrap();
    let mut second = SchemaRegistryBuilder::default();
    second.add_schema(b).unwrap();
    second.add_schema(a).unwrap();
    let first = first.finish().unwrap();
    let second = second.finish().unwrap();
    assert_eq!(first.schemas(), second.schemas());
    assert_eq!(
        first.schema_id(&id("pkg.core", "A")),
        Some(SchemaId::from_index(0))
    );
    assert_eq!(
        first.schema_id(&id("pkg.core", "B")),
        Some(SchemaId::from_index(1))
    );
}

#[test]
fn declaration_order_and_alias_resolution_are_preserved() {
    let target = id("defining.module", "Value");
    let alias = AliasIdentity::new("consumer.module", "ImportedValue");
    let mut builder = SchemaRegistryBuilder::default();
    builder
        .add_schema(record(
            target.clone(),
            vec![
                scalar("first", ScalarType::I32),
                FieldDraft::new("second", TypeExpr::Alias(alias.clone())),
            ],
        ))
        .unwrap();
    builder
        .add_alias(alias, TypeExpr::RecordRef(target.clone()))
        .unwrap();
    let schema = builder.finish().unwrap().schema(&target).unwrap().clone();
    assert_eq!(schema.fields()[0].name(), "first");
    assert!(matches!(
        schema.fields()[1].ty(),
        SemanticType::RecordRef(found) if found == &target
    ));
}

#[test]
fn record_reference_cycles_are_finite_and_valid() {
    let left = id("pkg", "Left");
    let right = id("pkg", "Right");
    let mut builder = SchemaRegistryBuilder::default();
    builder
        .add_schema(record(
            left.clone(),
            vec![FieldDraft::new("right", TypeExpr::RecordRef(right.clone()))],
        ))
        .unwrap();
    builder
        .add_schema(record(
            right.clone(),
            vec![FieldDraft::new("left", TypeExpr::RecordRef(left.clone()))],
        ))
        .unwrap();
    let registry = builder.finish().unwrap();
    assert_eq!(registry.schemas().len(), 2);
    assert!(matches!(
        registry.schema(&left).unwrap().fields()[0].ty(),
        SemanticType::RecordRef(found) if found == &right
    ));
}

#[test]
fn unknown_and_conflicting_schemas_fail_closed() {
    let owner = id("pkg", "Owner");
    let missing = id("pkg", "Missing");
    let mut unknown = SchemaRegistryBuilder::default();
    unknown
        .add_schema(record(
            owner.clone(),
            vec![FieldDraft::new(
                "missing",
                TypeExpr::RecordRef(missing.clone()),
            )],
        ))
        .unwrap();
    assert!(matches!(
        unknown.finish(),
        Err(SchemaError::UnknownSchema { identity }) if identity == missing
    ));

    let mut conflict = SchemaRegistryBuilder::default();
    conflict
        .add_schema(record(owner.clone(), vec![scalar("x", ScalarType::I64)]))
        .unwrap();
    assert!(matches!(
        conflict.add_schema(record(owner, vec![scalar("x", ScalarType::F64)])),
        Err(SchemaError::ConflictingSchema { .. })
    ));
}

#[test]
fn inline_record_and_alias_cycles_fail_without_expansion() {
    let owner = id("pkg", "Owner");
    let alias = AliasIdentity::new("pkg", "Loop");
    let mut inline = SchemaRegistryBuilder::default();
    inline
        .add_schema(record(
            owner.clone(),
            vec![FieldDraft::new(
                "nested",
                TypeExpr::InlineRecord { fields: Vec::new() },
            )],
        ))
        .unwrap();
    assert!(matches!(
        inline.finish(),
        Err(SchemaError::InlineRecordUnsupported)
    ));

    let mut aliases = SchemaRegistryBuilder::default();
    aliases
        .add_schema(record(
            owner,
            vec![FieldDraft::new("value", TypeExpr::Alias(alias.clone()))],
        ))
        .unwrap();
    aliases
        .add_alias(alias.clone(), TypeExpr::Alias(alias))
        .unwrap();
    assert!(matches!(
        aliases.finish(),
        Err(SchemaError::AliasCycle { .. })
    ));
}

#[test]
fn checked_limits_reject_extent_depth_and_fixed_elements() {
    let limits = RegistryLimits {
        max_schemas: 1,
        max_fields: 1,
        max_type_depth: 1,
        max_extent: 2,
        max_fixed_elements: 2,
        ..RegistryLimits::default()
    };
    let mut extent = SchemaRegistryBuilder::new(limits);
    assert!(matches!(
        extent.add_schema(record(
            id("pkg", "TooWide"),
            vec![FieldDraft::new(
                "values",
                TypeExpr::FixedArray {
                    element: Box::new(TypeExpr::Scalar(ScalarType::I64)),
                    extent: 3,
                },
            )],
        )),
        Err(SchemaError::ExtentLimitExceeded { .. })
    ));

    let mut elements = SchemaRegistryBuilder::new(RegistryLimits {
        max_fixed_elements: 1,
        ..limits
    });
    elements
        .add_schema(record(
            id("pkg", "TooMany"),
            vec![FieldDraft::new(
                "values",
                TypeExpr::FixedArray {
                    element: Box::new(TypeExpr::Scalar(ScalarType::I64)),
                    extent: 2,
                },
            )],
        ))
        .unwrap();
    assert!(matches!(
        elements.finish(),
        Err(SchemaError::FixedElementLimitExceeded { .. })
    ));

    let mut depth = SchemaRegistryBuilder::new(limits);
    assert!(matches!(
        depth.add_schema(record(
            id("pkg", "TooDeep"),
            vec![FieldDraft::new(
                "values",
                TypeExpr::FixedArray {
                    element: Box::new(TypeExpr::FixedArray {
                        element: Box::new(TypeExpr::Scalar(ScalarType::I64)),
                        extent: 1,
                    }),
                    extent: 1,
                },
            )],
        )),
        Err(SchemaError::TypeDepthExceeded { .. })
    ));
}

#[test]
fn duplicate_fields_and_path_like_identity_components_are_rejected() {
    let mut duplicate = SchemaRegistryBuilder::default();
    assert!(matches!(
        duplicate.add_schema(record(
            id("pkg", "Pair"),
            vec![scalar("x", ScalarType::I64), scalar("x", ScalarType::I64)],
        )),
        Err(SchemaError::DuplicateField { .. })
    ));

    let mut path = SchemaRegistryBuilder::default();
    assert!(matches!(
        path.add_schema(record(
            SchemaIdentity::new("/tmp/project", "Bad"),
            Vec::new(),
        )),
        Err(SchemaError::PathLikeIdentity { .. })
    ));

    for identity in [
        SchemaIdentity::new("pkg", "nested/Bad"),
        SchemaIdentity::new("pkg..shadow", "Bad"),
        SchemaIdentity::new("pkg", "Bad..shadow"),
    ] {
        let mut path = SchemaRegistryBuilder::default();
        assert!(matches!(
            path.add_schema(record(identity, Vec::new())),
            Err(SchemaError::PathLikeIdentity { .. })
        ));
    }

    let mut empty_field = SchemaRegistryBuilder::default();
    assert!(matches!(
        empty_field.add_schema(record(id("pkg", "Bad"), vec![scalar("", ScalarType::I64)],)),
        Err(SchemaError::EmptyField { .. })
    ));
}

#[test]
fn rejected_duplicate_alias_does_not_replace_original_target() {
    let owner = id("pkg", "Owner");
    let alias = AliasIdentity::new("pkg", "Value");
    let mut builder = SchemaRegistryBuilder::default();
    builder
        .add_schema(record(
            owner,
            vec![FieldDraft::new("value", TypeExpr::Alias(alias.clone()))],
        ))
        .unwrap();
    builder
        .add_alias(alias.clone(), TypeExpr::Scalar(ScalarType::I64))
        .unwrap();
    assert!(matches!(
        builder.add_alias(alias, TypeExpr::Scalar(ScalarType::F64)),
        Err(SchemaError::DuplicateAlias { .. })
    ));
    let registry = builder.finish().unwrap();
    assert!(matches!(
        registry.schemas()[0].fields()[0].ty(),
        SemanticType::Scalar(ScalarType::I64)
    ));
}

#[test]
fn unused_aliases_are_checked_and_dynamic_arrays_remain_dynamic() {
    let owner = id("pkg", "Owner");
    let unused = AliasIdentity::new("pkg", "Unused");
    let missing = id("pkg", "Missing");
    let mut dangling = SchemaRegistryBuilder::default();
    dangling
        .add_schema(record(owner.clone(), Vec::new()))
        .unwrap();
    dangling
        .add_alias(unused, TypeExpr::RecordRef(missing.clone()))
        .unwrap();
    assert!(matches!(
        dangling.finish(),
        Err(SchemaError::UnknownSchema { identity }) if identity == missing
    ));

    let mut dynamic = SchemaRegistryBuilder::default();
    dynamic
        .add_schema(record(
            owner.clone(),
            vec![FieldDraft::new(
                "values",
                TypeExpr::DynamicArray {
                    element: Box::new(TypeExpr::Scalar(ScalarType::I64)),
                },
            )],
        ))
        .unwrap();
    let registry = dynamic.finish().unwrap();
    assert!(matches!(
        registry.schema(&owner).unwrap().fields()[0].ty(),
        SemanticType::DynamicArray { element }
            if matches!(element.as_ref(), SemanticType::Scalar(ScalarType::I64))
    ));
}

#[test]
fn generic_schema_identity_is_rejected_until_typed_args_exist() {
    let generic = SchemaIdentity::new("pkg", "Box").with_type_args(["i64"]);
    let mut builder = SchemaRegistryBuilder::default();
    assert!(matches!(
        builder.add_schema(record(generic.clone(), Vec::new())),
        Err(SchemaError::UnsupportedTypeArguments { identity }) if identity == generic
    ));
}

#[test]
fn alias_and_identity_limits_are_checked_before_insertion() {
    let owner = id("pkg", "Owner");
    let alias = AliasIdentity::new("pkg", "Value");
    let mut aliases = SchemaRegistryBuilder::new(RegistryLimits {
        max_aliases: 1,
        ..RegistryLimits::default()
    });
    aliases
        .add_alias(alias.clone(), TypeExpr::Scalar(ScalarType::I64))
        .unwrap();
    assert!(matches!(
        aliases.add_alias(
            AliasIdentity::new("pkg", "Other"),
            TypeExpr::Scalar(ScalarType::I64),
        ),
        Err(SchemaError::AliasLimitExceeded { .. })
    ));
    aliases
        .add_schema(record(
            owner,
            vec![FieldDraft::new("value", TypeExpr::Alias(alias))],
        ))
        .unwrap();
    assert!(aliases.finish().is_ok());

    let mut identity = SchemaRegistryBuilder::new(RegistryLimits {
        max_identity_bytes: 8,
        ..RegistryLimits::default()
    });
    assert!(matches!(
        identity.add_schema(record(id("long-owner", "Thing"), Vec::new())),
        Err(SchemaError::IdentityBytesExceeded { .. })
    ));
}

#[test]
fn schema_field_and_zero_extent_controls_are_explicit() {
    let mut schemas = SchemaRegistryBuilder::new(RegistryLimits {
        max_schemas: 1,
        max_fields: 1,
        ..RegistryLimits::default()
    });
    schemas
        .add_schema(record(id("pkg", "One"), vec![scalar("x", ScalarType::I64)]))
        .unwrap();
    assert!(matches!(
        schemas.add_schema(record(id("pkg", "Two"), Vec::new())),
        Err(SchemaError::SchemaLimitExceeded { .. })
    ));

    let mut fields = SchemaRegistryBuilder::new(RegistryLimits {
        max_fields: 1,
        ..RegistryLimits::default()
    });
    assert!(matches!(
        fields.add_schema(record(
            id("pkg", "Wide"),
            vec![scalar("x", ScalarType::I64), scalar("y", ScalarType::I64)],
        )),
        Err(SchemaError::FieldLimitExceeded { .. })
    ));

    let owner = id("pkg", "Empty");
    let mut zero = SchemaRegistryBuilder::default();
    zero.add_schema(record(
        owner.clone(),
        vec![FieldDraft::new(
            "values",
            TypeExpr::FixedArray {
                element: Box::new(TypeExpr::Scalar(ScalarType::I64)),
                extent: 0,
            },
        )],
    ))
    .unwrap();
    let registry = zero.finish().unwrap();
    assert!(matches!(
        registry.schema(&owner).unwrap().fields()[0].ty(),
        SemanticType::FixedArray { extent: 0, .. }
    ));
}

#[test]
fn nested_element_limits_survive_dynamic_and_empty_containers() {
    let fixed = |extent| TypeExpr::FixedArray {
        element: Box::new(TypeExpr::Scalar(ScalarType::I64)),
        extent,
    };
    let finish = |ty| {
        let mut builder = SchemaRegistryBuilder::new(RegistryLimits {
            max_fixed_elements: 4,
            ..RegistryLimits::default()
        });
        builder
            .add_schema(record(
                id("pkg", "Owner"),
                vec![FieldDraft::new("items", ty)],
            ))
            .unwrap();
        builder.finish()
    };
    for element in [fixed(5), fixed(3)] {
        let result = finish(TypeExpr::DynamicArray {
            element: Box::new(element.clone()),
        });
        if matches!(element, TypeExpr::FixedArray { extent: 5, .. }) {
            assert!(matches!(
                result,
                Err(SchemaError::FixedElementLimitExceeded { limit: 4 })
            ));
        } else {
            assert!(result.is_ok()); // descriptor + three elements exactly fills the limit
        }
    }
    assert!(matches!(
        finish(TypeExpr::FixedArray {
            element: Box::new(fixed(5)),
            extent: 0
        }),
        Err(SchemaError::FixedElementLimitExceeded { limit: 4 })
    ));
    assert!(
        finish(TypeExpr::FixedArray {
            element: Box::new(fixed(4)),
            extent: 0
        })
        .is_ok()
    );
}
