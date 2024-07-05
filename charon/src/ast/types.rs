pub use super::types_utils::*;
use crate::ast::{AttrInfo, ItemMeta, Literal, ScalarValue, Span, TraitItemName};
use crate::ids::Vector;
use derivative::Derivative;
use derive_visitor::{Drive, DriveMut, Event, Visitor, VisitorMut};
use macros::{EnumAsGetters, EnumIsA, EnumToGetters, VariantIndexArity, VariantName};
use serde::{Deserialize, Serialize};

pub type FieldName = String;

// We need to manipulate a lot of indices for the types, variables, definitions,
// etc. In order not to confuse them, we define an index type for every one of
// them (which is just a struct with a unique usize field), together with some
// utilities like a fresh index generator. Those structures and utilities are
// generated by using macros.
generate_index_type!(TypeVarId, "T");
generate_index_type!(TypeDeclId, "Adt");
generate_index_type!(VariantId, "Variant");
generate_index_type!(FieldId, "Field");
generate_index_type!(RegionId, "Region");
generate_index_type!(ConstGenericVarId, "Const");
generate_index_type!(GlobalDeclId, "Global");

/// Type variable.
/// We make sure not to mix variables and type variables by having two distinct
/// definitions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Drive, DriveMut)]
pub struct TypeVar {
    /// Unique index identifying the variable
    pub index: TypeVarId,
    /// Variable name
    pub name: String,
}

/// Region variable.
#[derive(
    Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Hash, PartialOrd, Ord, Drive, DriveMut,
)]
pub struct RegionVar {
    /// Unique index identifying the variable
    pub index: RegionId,
    /// Region name
    pub name: Option<String>,
}

/// Const Generic Variable
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Drive, DriveMut)]
pub struct ConstGenericVar {
    /// Unique index identifying the variable
    pub index: ConstGenericVarId,
    /// Const generic name
    pub name: String,
    /// Type of the const generic
    pub ty: LiteralTy,
}

#[derive(
    Debug,
    PartialEq,
    Eq,
    Copy,
    Clone,
    Hash,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
    Drive,
    DriveMut,
)]
#[serde(transparent)]
pub struct DeBruijnId {
    pub index: usize,
}

#[derive(
    Debug,
    PartialEq,
    Eq,
    Copy,
    Clone,
    Hash,
    PartialOrd,
    Ord,
    EnumIsA,
    EnumAsGetters,
    Serialize,
    Deserialize,
    Drive,
    DriveMut,
)]
#[charon::variants_prefix("R")]
pub enum Region {
    /// Static region
    Static,
    /// Bound region variable.
    ///
    /// **Important**:
    /// ==============
    /// Similarly to what the Rust compiler does, we use De Bruijn indices to
    /// identify *groups* of bound variables, and variable identifiers to
    /// identity the variables inside the groups.
    ///
    /// For instance, we have the following:
    /// ```text
    ///                     we compute the De Bruijn indices from here
    ///                            VVVVVVVVVVVVVVVVVVVVVVV
    /// fn f<'a, 'b>(x: for<'c> fn(&'a u8, &'b u16, &'c u32) -> u64) {}
    ///      ^^^^^^         ^^       ^       ^        ^
    ///        |      De Bruijn: 0   |       |        |
    ///  De Bruijn: 1                |       |        |
    ///                        De Bruijn: 1  |    De Bruijn: 0
    ///                           Var id: 0  |       Var id: 0
    ///                                      |
    ///                                De Bruijn: 1
    ///                                   Var id: 1
    /// ```
    BVar(DeBruijnId, RegionId),
    /// Erased region
    Erased,
    /// For error reporting.
    #[charon::opaque]
    Unknown,
}

/// Identifier of a trait instance.
/// This is derived from the trait resolution.
///
/// Should be read as a path inside the trait clauses which apply to the current
/// definition. Note that every path designated by [TraitInstanceId] refers
/// to a *trait instance*, which is why the [Clause] variant may seem redundant
/// with some of the other variants.
#[derive(
    Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Ord, PartialOrd, Drive, DriveMut,
)]
pub enum TraitInstanceId {
    /// A specific top-level implementation item.
    TraitImpl(TraitImplId),

    /// One of the local clauses.
    ///
    /// Example:
    /// ```text
    /// fn f<T>(...) where T : Foo
    ///                    ^^^^^^^
    ///                    Clause(0)
    /// ```
    Clause(TraitClauseId),

    /// A parent clause
    ///
    /// Remark: the [TraitDeclId] gives the trait declaration which is
    /// implemented by the instance id from which we take the parent clause
    /// (see example below). It is not necessary and included for convenience.
    ///
    /// Example:
    /// ```text
    /// trait Foo1 {}
    /// trait Foo2 { fn f(); }
    ///
    /// trait Bar : Foo1 + Foo2 {}
    ///             ^^^^   ^^^^
    ///                    parent clause 1
    ///     parent clause 0
    ///
    /// fn g<T : Bar>(x : T) {
    ///   x.f()
    ///   ^^^^^
    ///   Parent(Clause(0), Bar, 1)::f(x)
    ///                          ^
    ///                          parent clause 1 of clause 0
    ///                     ^^^
    ///              clause 0 implements Bar
    /// }
    /// ```
    ParentClause(Box<TraitInstanceId>, TraitDeclId, TraitClauseId),

    /// A clause bound in a trait item (typically a trait clause in an
    /// associated type).
    ///
    /// Remark: the [TraitDeclId] gives the trait declaration which is
    /// implemented by the trait implementation from which we take the item
    /// (see below). It is not necessary and provided for convenience.
    ///
    /// Example:
    /// ```text
    /// trait Foo {
    ///   type W: Bar0 + Bar1 // Bar1 contains a method bar1
    ///                  ^^^^
    ///               this is the clause 1 applying to W
    /// }
    ///
    /// fn f<T : Foo>(x : T::W) {
    ///   x.bar1();
    ///   ^^^^^^^
    ///   ItemClause(Clause(0), Foo, W, 1)
    ///                              ^^^^
    ///                              clause 1 from item W (from local clause 0)
    ///                         ^^^
    ///                local clause 0 implements Foo
    /// }
    /// ```
    ///
    ///
    ItemClause(
        Box<TraitInstanceId>,
        TraitDeclId,
        TraitItemName,
        TraitClauseId,
    ),

    /// Self, in case of trait declarations/implementations.
    ///
    /// Putting [Self] at the end on purpose, so that when ordering the clauses
    /// we start with the other clauses (in particular, the local clauses). It
    /// is useful to give priority to the local clauses when solving the trait
    /// obligations which are fullfilled by the trait parameters.
    #[charon::rename("Self")]
    SelfId,

    /// A specific builtin trait implementation like [core::marker::Sized] or
    /// auto trait implementation like [core::marker::Syn].
    BuiltinOrAuto(TraitDeclId),

    /// The automatically-generated implementation for `dyn Trait`.
    Dyn(TraitDeclId),

    /// For error reporting.
    #[charon::rename("UnknownTrait")]
    Unknown(String),
}

/// A reference to a trait
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Drive, DriveMut)]
pub struct TraitRef {
    pub trait_id: TraitInstanceId,
    pub generics: GenericArgs,
    /// Not necessary, but useful
    pub trait_decl_ref: TraitDeclRef,
}

/// Reference to a trait declaration.
///
/// About the generics, if we write:
/// ```text
/// impl Foo<bool> for String { ... }
/// ```
///
/// The substitution is: `[String, bool]`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Drive, DriveMut)]
pub struct TraitDeclRef {
    #[charon::rename("trait_decl_id")]
    pub trait_id: TraitDeclId,
    #[charon::rename("decl_generics")]
    pub generics: GenericArgs,
}

/// .0 outlives .1
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OutlivesPred<T, U>(pub T, pub U);

// The derive macro doesn't handle generics well.
impl<T: Drive, U: Drive> Drive for OutlivesPred<T, U> {
    fn drive<V: Visitor>(&self, visitor: &mut V) {
        visitor.visit(self, Event::Enter);
        self.0.drive(visitor);
        self.1.drive(visitor);
        visitor.visit(self, Event::Exit);
    }
}
impl<T: DriveMut, U: DriveMut> DriveMut for OutlivesPred<T, U> {
    fn drive_mut<V: VisitorMut>(&mut self, visitor: &mut V) {
        visitor.visit(self, Event::Enter);
        self.0.drive_mut(visitor);
        self.1.drive_mut(visitor);
        visitor.visit(self, Event::Exit);
    }
}

pub type RegionOutlives = OutlivesPred<Region, Region>;
pub type TypeOutlives = OutlivesPred<Ty, Region>;

/// A constraint over a trait associated type.
///
/// Example:
/// ```text
/// T : Foo<S = String>
///         ^^^^^^^^^^
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Drive, DriveMut)]
pub struct TraitTypeConstraint {
    pub trait_ref: TraitRef,
    pub type_name: TraitItemName,
    pub ty: Ty,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, Hash, Drive, DriveMut)]
pub struct GenericArgs {
    pub regions: Vec<Region>,
    pub types: Vec<Ty>,
    pub const_generics: Vec<ConstGeneric>,
    // TODO: rename to match [GenericParams]?
    pub trait_refs: Vec<TraitRef>,
}

/// Generic parameters for a declaration.
/// We group the generics which come from the Rust compiler substitutions
/// (the regions, types and const generics) as well as the trait clauses.
/// The reason is that we consider that those are parameters that need to
/// be filled. We group in a different place the predicates which are not
/// trait clauses, because those enforce constraints but do not need to
/// be filled with witnesses/instances.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize, Drive, DriveMut)]
pub struct GenericParams {
    pub regions: Vector<RegionId, RegionVar>,
    pub types: Vector<TypeVarId, TypeVar>,
    pub const_generics: Vector<ConstGenericVarId, ConstGenericVar>,
    // TODO: rename to match [GenericArgs]?
    pub trait_clauses: Vector<TraitClauseId, TraitClause>,
    /// The first region in the pair outlives the second region
    pub regions_outlive: Vec<RegionOutlives>,
    /// The type outlives the region
    pub types_outlive: Vec<TypeOutlives>,
    /// Constraints over trait associated types
    pub trait_type_constraints: Vec<TraitTypeConstraint>,
}

/// A predicate of the form `exists<T> where T: Trait`.
///
/// TODO: store something useful here
#[derive(Debug, Default, Clone, Hash, PartialEq, Eq, Serialize, Deserialize, Drive, DriveMut)]
pub struct ExistentialPredicate;

generate_index_type!(TraitClauseId, "TraitClause");
generate_index_type!(TraitDeclId, "TraitDecl");
generate_index_type!(TraitImplId, "TraitImpl");

/// A predicate of the form `Type: Trait<Args>`.
#[derive(Debug, Clone, Serialize, Deserialize, Derivative, Drive, DriveMut)]
#[derivative(PartialEq)]
pub struct TraitClause {
    /// We use this id when solving trait constraints, to be able to refer
    /// to specific where clauses when the selected trait actually is linked
    /// to a parameter.
    pub clause_id: TraitClauseId,
    #[derivative(PartialEq = "ignore")]
    pub span: Option<Span>,
    /// Where the predicate was written, relative to the item that requires it.
    #[derivative(PartialEq = "ignore")]
    #[charon::opaque]
    pub origin: PredicateOrigin,
    /// The trait that is implemented.
    pub trait_id: TraitDeclId,
    /// The generics applied to the trait. Note: this includes the `Self` type.
    /// Remark: the trait refs list in the [generics] field should be empty.
    #[charon::rename("clause_generics")]
    pub generics: GenericArgs,
}

impl Eq for TraitClause {}

/// Where a given predicate came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Derivative, Drive, DriveMut)]
pub enum PredicateOrigin {
    // Note: we use this for globals too, but that's only available with an unstable feature.
    // ```
    // fn function<T: Clone>() {}
    // fn function<T>() where T: Clone {}
    // const NONE<T: Copy>: Option<T> = None;
    // ```
    WhereClauseOnFn,
    // ```
    // struct Struct<T: Clone> {}
    // struct Struct<T> where T: Clone {}
    // type TypeAlias<T: Clone> = ...;
    // ```
    WhereClauseOnType,
    // Note: this is both trait impls and inherent impl blocks.
    // ```
    // impl<T: Clone> Type<T> {}
    // impl<T> Type<T> where T: Clone {}
    // impl<T> Trait for Type<T> where T: Clone {}
    // ```
    WhereClauseOnImpl,
    // The special `Self: Trait` clause which is in scope inside the definition of `Foo` or an
    // implementation of it.
    // ```
    // trait Trait {}
    // ```
    TraitSelf,
    // Note: this also includes supertrait constraings.
    // ```
    // trait Trait<T: Clone> {}
    // trait Trait<T> where T: Clone {}
    // trait Trait: Clone {}
    // ```
    WhereClauseOnTrait,
    // ```
    // trait Trait {
    //     type AssocType: Clone;
    // }
    // ```
    TraitItem(TraitItemName),
}

/// A type declaration.
///
/// Types can be opaque or transparent.
///
/// Transparent types are local types not marked as opaque.
/// Opaque types are the others: local types marked as opaque, and non-local
/// types (coming from external dependencies).
///
/// In case the type is transparent, the declaration also contains the
/// type definition (see [TypeDeclKind]).
///
/// A type can only be an ADT (structure or enumeration), as type aliases are
/// inlined in MIR.
#[derive(Debug, Clone, Serialize, Deserialize, Drive, DriveMut)]
pub struct TypeDecl {
    #[drive(skip)]
    pub def_id: TypeDeclId,
    /// Meta information associated with the item.
    pub item_meta: ItemMeta,
    pub generics: GenericParams,
    /// The type kind: enum, struct, or opaque.
    pub kind: TypeDeclKind,
}

#[derive(Debug, Clone, EnumIsA, EnumAsGetters, Serialize, Deserialize, Drive, DriveMut)]
pub enum TypeDeclKind {
    Struct(Vector<FieldId, Field>),
    Enum(Vector<VariantId, Variant>),
    /// An opaque type.
    ///
    /// Either a local type marked as opaque, or an external type.
    Opaque,
    /// An alias to another type. This only shows up in the top-level list of items, as rustc
    /// inlines uses of type aliases everywhere else.
    Alias(Ty),
    /// Used if an error happened during the extraction, and we don't panic
    /// on error.
    #[charon::opaque]
    Error(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, Drive, DriveMut)]
pub struct Variant {
    pub span: Span,
    pub attr_info: AttrInfo,
    #[charon::rename("variant_name")]
    pub name: String,
    pub fields: Vector<FieldId, Field>,
    /// The discriminant used at runtime. This is used in `remove_read_discriminant` to match up
    /// `SwitchInt` targets with the corresponding `Variant`.
    pub discriminant: ScalarValue,
}

#[derive(Debug, Clone, Serialize, Deserialize, Drive, DriveMut)]
pub struct Field {
    pub span: Span,
    pub attr_info: AttrInfo,
    #[charon::rename("field_name")]
    pub name: Option<String>,
    #[charon::rename("field_ty")]
    pub ty: Ty,
}

#[derive(
    Debug,
    PartialEq,
    Eq,
    Copy,
    Clone,
    EnumIsA,
    VariantName,
    Serialize,
    Deserialize,
    Drive,
    DriveMut,
    Hash,
    Ord,
    PartialOrd,
)]
#[charon::rename("IntegerType")]
pub enum IntegerTy {
    Isize,
    I8,
    I16,
    I32,
    I64,
    I128,
    Usize,
    U8,
    U16,
    U32,
    U64,
    U128,
}

#[derive(
    Debug,
    PartialEq,
    Eq,
    Clone,
    Copy,
    Hash,
    VariantName,
    EnumIsA,
    Serialize,
    Deserialize,
    Drive,
    DriveMut,
    Ord,
    PartialOrd,
)]
#[charon::variants_prefix("R")]
pub enum RefKind {
    Mut,
    Shared,
}

/// Type identifier.
///
/// Allows us to factorize the code for assumed types, adts and tuples
#[derive(
    Debug,
    PartialEq,
    Eq,
    Clone,
    Copy,
    VariantName,
    EnumAsGetters,
    EnumIsA,
    Serialize,
    Deserialize,
    Drive,
    DriveMut,
    Hash,
    Ord,
    PartialOrd,
)]
#[charon::variants_prefix("T")]
pub enum TypeId {
    /// A "regular" ADT type.
    ///
    /// Includes transparent ADTs and opaque ADTs (local ADTs marked as opaque,
    /// and external ADTs).
    #[charon::rename("TAdtId")]
    Adt(TypeDeclId),
    Tuple,
    /// Assumed type. Either a primitive type like array or slice, or a
    /// non-primitive type coming from a standard library
    /// and that we handle like a primitive type. Types falling into this
    /// category include: Box, Vec, Cell...
    /// The Array and Slice types were initially modelled as primitive in
    /// the [Ty] type. We decided to move them to assumed types as it allows
    /// for more uniform treatment throughout the codebase.
    Assumed(AssumedTy),
}

/// Types of primitive values. Either an integer, bool, char
#[derive(
    Debug,
    PartialEq,
    Eq,
    Clone,
    Copy,
    VariantName,
    EnumIsA,
    EnumAsGetters,
    VariantIndexArity,
    Serialize,
    Deserialize,
    Drive,
    DriveMut,
    Hash,
    Ord,
    PartialOrd,
)]
#[charon::rename("LiteralType")]
#[charon::variants_prefix("T")]
pub enum LiteralTy {
    Integer(IntegerTy),
    Bool,
    Char,
}

/// Const Generic Values. Either a primitive value, or a variable corresponding to a primitve value
#[derive(
    Debug,
    PartialEq,
    Eq,
    Clone,
    VariantName,
    EnumIsA,
    EnumAsGetters,
    VariantIndexArity,
    Serialize,
    Deserialize,
    Drive,
    DriveMut,
    Hash,
    Ord,
    PartialOrd,
)]
#[charon::variants_prefix("Cg")]
pub enum ConstGeneric {
    /// A global constant
    Global(GlobalDeclId),
    /// A const generic variable
    Var(ConstGenericVarId),
    /// A concrete value
    Value(Literal),
}

/// A type.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    VariantName,
    EnumIsA,
    EnumAsGetters,
    EnumToGetters,
    VariantIndexArity,
    Serialize,
    Deserialize,
    Drive,
    DriveMut,
)]
#[charon::variants_prefix("T")]
pub enum Ty {
    /// An ADT.
    /// Note that here ADTs are very general. They can be:
    /// - user-defined ADTs
    /// - tuples (including `unit`, which is a 0-tuple)
    /// - assumed types (includes some primitive types, e.g., arrays or slices)
    /// The information on the nature of the ADT is stored in (`TypeId`)[TypeId].
    /// The last list is used encode const generics, e.g., the size of an array
    Adt(TypeId, GenericArgs),
    #[charon::rename("TVar")]
    TypeVar(TypeVarId),
    Literal(LiteralTy),
    /// The never type, for computations which don't return. It is sometimes
    /// necessary for intermediate variables. For instance, if we do (coming
    /// from the rust documentation):
    /// ```text
    /// let num: u32 = match get_a_number() {
    ///     Some(num) => num,
    ///     None => break,
    /// };
    /// ```
    /// the second branch will have type `Never`. Also note that `Never`
    /// can be coerced to any type.
    ///
    /// Note that we eliminate the variables which have this type in a micro-pass.
    /// As statements don't have types, this type disappears eventually disappears
    /// from the AST.
    Never,
    // We don't support floating point numbers on purpose (for now)
    /// A borrow
    Ref(Region, Box<Ty>, RefKind),
    /// A raw pointer.
    RawPtr(Box<Ty>, RefKind),
    /// A trait associated type
    ///
    /// Ex.:
    /// ```text
    /// trait Foo {
    ///   type Bar; // type associated to the trait Foo
    /// }
    /// ```
    TraitType(TraitRef, TraitItemName),
    /// `dyn Trait`
    ///
    /// This carries an existentially quantified list of predicates, e.g. `exists<T> where T:
    /// Into<u64>`. The predicate must quantify over a single type and no any regions or constants.
    ///
    /// TODO: we don't translate this properly yet.
    DynTrait(ExistentialPredicate),
    /// Arrow type, used in particular for the local function pointers.
    /// This is essentially a "constrained" function signature:
    /// arrow types can only contain generic lifetime parameters
    /// (no generic types), no predicates, etc.
    Arrow(Vector<RegionId, RegionVar>, Vec<Ty>, Box<Ty>),
}

/// Assumed types identifiers.
///
/// WARNING: for now, all the assumed types are covariant in the generic
/// parameters (if there are). Adding types which don't satisfy this
/// will require to update the code abstracting the signatures (to properly
/// take into account the lifetime constraints).
///
/// TODO: update to not hardcode the types (except `Box` maybe) and be more
/// modular.
/// TODO: move to assumed.rs?
#[derive(
    Debug,
    PartialEq,
    Eq,
    Clone,
    Copy,
    EnumIsA,
    EnumAsGetters,
    VariantName,
    Serialize,
    Deserialize,
    Drive,
    DriveMut,
    Hash,
    Ord,
    PartialOrd,
)]
#[charon::variants_prefix("T")]
pub enum AssumedTy {
    /// Boxes have a special treatment: we translate them as identity.
    Box,
    /// Comes from the standard library. See the comments for [Ty::RawPtr]
    /// as to why we have this here.
    #[charon::opaque]
    PtrUnique,
    /// Same comments as for [AssumedTy::PtrUnique]
    #[charon::opaque]
    PtrNonNull,
    /// Primitive type
    Array,
    /// Primitive type
    Slice,
    /// Primitive type
    Str,
}

/// We use this to store information about the parameters in parent blocks.
/// This is necessary because in the definitions we store *all* the generics,
/// including those coming from the outer impl block.
///
/// For instance:
/// ```text
/// impl Foo<T> {
///         ^^^
///       outer block generics
///   fn bar<U>(...)  { ... }
///         ^^^
///       generics local to the function bar
/// }
/// ```
///
/// In `bar` we store the generics: `[T, U]`.
///
/// We however sometimes need to make a distinction between those two kinds
/// of generics, in particular when manipulating traits. For instance:
///
/// ```text
/// impl<T> Foo for Bar<T> {
///   fn baz<U>(...)  { ... }
/// }
///
/// fn test(...) {
///    x.baz(...); // Here, we refer to the call as:
///                // > Foo<T>::baz<U>(...)
///                // If baz hadn't been a method implementation of a trait,
///                // we would have refered to it as:
///                // > baz<T, U>(...)
///                // The reason is that with traits, we refer to the whole
///                // trait implementation (as if it were a structure), then
///                // pick a specific method inside (as if projecting a field
///                // from a structure).
/// }
/// ```
///
/// **Remark**: Rust only allows refering to the generics of the immediately
/// outer block. For this reason, when we need to store the information about
/// the generics of the outer block(s), we need to do it only for one level
/// (this definitely makes things simpler).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Drive, DriveMut)]
pub struct ParamsInfo {
    pub num_region_params: usize,
    pub num_type_params: usize,
    pub num_const_generic_params: usize,
    pub num_trait_clauses: usize,
    pub num_regions_outlive: usize,
    pub num_types_outlive: usize,
    pub num_trait_type_constraints: usize,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize, Drive, DriveMut)]
pub enum ClosureKind {
    Fn,
    FnMut,
    FnOnce,
}

/// Additional information for closures.
/// We mostly use it in micro-passes like [crate::update_closure_signature].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Drive, DriveMut)]
pub struct ClosureInfo {
    pub kind: ClosureKind,
    /// Contains the types of the fields in the closure state.
    /// More precisely, for every place captured by the
    /// closure, the state has one field (typically a ref).
    ///
    /// For instance, below the closure has a state with two fields of type `&u32`:
    /// ```text
    /// pub fn test_closure_capture(x: u32, y: u32) -> u32 {
    ///   let f = &|z| x + y + z;
    ///   (f)(0)
    /// }
    /// ```
    pub state: Vec<Ty>,
}

/// A function signature.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Drive, DriveMut)]
pub struct FunSig {
    /// Is the function unsafe or not
    pub is_unsafe: bool,
    /// `true` if the signature is for a closure.
    ///
    /// Importantly: if the signature is for a closure, then:
    /// - the type and const generic params actually come from the parent function
    ///   (the function in which the closure is defined)
    /// - the region variables are local to the closure
    pub is_closure: bool,
    /// Additional information if this is the signature of a closure.
    pub closure_info: Option<ClosureInfo>,
    pub generics: GenericParams,
    /// Optional fields, for trait methods only (see the comments in [ParamsInfo]).
    pub parent_params_info: Option<ParamsInfo>,
    pub inputs: Vec<Ty>,
    pub output: Ty,
}
