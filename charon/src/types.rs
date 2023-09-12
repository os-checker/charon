#![allow(dead_code)]

use crate::meta::Meta;
use crate::names::TypeName;
use crate::regions_hierarchy::RegionGroups;
pub use crate::types_utils::*;
use crate::values::Literal;
use macros::{
    generate_index_type, EnumAsGetters, EnumIsA, EnumToGetters, VariantIndexArity, VariantName,
};
use serde::Serialize;

pub type FieldName = String;

// We need to manipulate a lot of indices for the types, variables, definitions,
// etc. In order not to confuse them, we define an index type for every one of
// them (which is just a struct with a unique usize field), together with some
// utilities like a fresh index generator. Those structures and utilities are
// generated by using macros.
generate_index_type!(TypeVarId);
generate_index_type!(TypeDeclId);
generate_index_type!(VariantId);
generate_index_type!(FieldId);
generate_index_type!(RegionVarId);
generate_index_type!(ConstGenericVarId);
generate_index_type!(GlobalDeclId);

/// Type variable.
/// We make sure not to mix variables and type variables by having two distinct
/// definitions.
#[derive(Debug, Clone, Serialize)]
pub struct TypeVar {
    /// Unique index identifying the variable
    pub index: TypeVarId::Id,
    /// Variable name
    pub name: String,
}

/// Region variable.
#[derive(Debug, Clone, Serialize)]
pub struct RegionVar {
    /// Unique index identifying the variable
    pub index: RegionVarId::Id,
    /// Region name
    pub name: Option<String>,
}

/// Const Generic Variable
#[derive(Debug, Clone, Serialize)]
pub struct ConstGenericVar {
    /// Unique index identifying the variable
    pub index: ConstGenericVarId::Id,
    /// Const generic name
    pub name: String,
    /// Type of the const generic
    pub ty: LiteralTy,
}

/// Region as used in a function's signatures (in which case we use region variable
/// ids) and in symbolic variables and projections (in which case we use region
/// ids).
#[derive(
    Debug, PartialEq, Eq, Clone, Copy, Hash, PartialOrd, Ord, EnumIsA, EnumAsGetters, Serialize,
)]
pub enum Region<Rid: Copy + Eq> {
    /// Static region
    Static,
    /// Non-static region.
    Var(Rid),
}

/// The type of erased regions. See [`Ty`](Ty) for more explanations.
/// We could use `()`, but having a dedicated type makes things more explicit.
#[derive(Debug, PartialEq, Eq, Clone, Copy, Serialize)]
pub enum ErasedRegion {
    Erased,
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
#[derive(Debug, Clone, Serialize)]
pub struct TypeDecl {
    pub def_id: TypeDeclId::Id,
    /// Meta information associated with the type.
    pub meta: Meta,
    pub name: TypeName,
    pub region_params: RegionVarId::Vector<RegionVar>,
    pub type_params: TypeVarId::Vector<TypeVar>,
    pub const_generic_params: ConstGenericVarId::Vector<ConstGenericVar>,
    /// The type kind: enum, struct, or opaque.
    pub kind: TypeDeclKind,
    /// The lifetime's hierarchy between the different regions.
    /// We initialize it to a dummy value, then compute it once the whole crate
    /// has been translated.
    ///
    /// TODO: move to Aeneas
    pub regions_hierarchy: RegionGroups,
}

#[derive(Debug, Clone, EnumIsA, EnumAsGetters, Serialize)]
pub enum TypeDeclKind {
    Struct(FieldId::Vector<Field>),
    Enum(VariantId::Vector<Variant>),
    /// An opaque type.
    ///
    /// Either a local type marked as opaque, or an external type.
    Opaque,
}

#[derive(Debug, Clone, Serialize)]
pub struct Variant {
    pub meta: Meta,
    pub name: String,
    pub fields: FieldId::Vector<Field>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Field {
    pub meta: Meta,
    pub name: Option<String>,
    pub ty: RTy,
}

#[derive(Debug, PartialEq, Eq, Copy, Clone, EnumIsA, VariantName, Serialize)]
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

#[derive(Debug, PartialEq, Eq, Clone, Copy, VariantName, EnumIsA, Serialize)]
pub enum RefKind {
    Mut,
    Shared,
}

/// Type identifier.
///
/// Allows us to factorize the code for assumed types, adts and tuples
#[derive(Debug, PartialEq, Eq, Clone, VariantName, EnumAsGetters, EnumIsA, Serialize)]
pub enum TypeId {
    /// A "regular" ADT type.
    ///
    /// Includes transparent ADTs and opaque ADTs (local ADTs marked as opaque,
    /// and external ADTs).
    Adt(TypeDeclId::Id),
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

pub type TypeDecls = TypeDeclId::Map<TypeDecl>;

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
)]
pub enum LiteralTy {
    Integer(IntegerTy),
    Bool,
    Char,
}

/// Const Generic Values. Either a primitive value, or a variable corresponding to a primitve value
#[derive(
    Debug, PartialEq, Eq, Clone, VariantName, EnumIsA, EnumAsGetters, VariantIndexArity, Serialize,
)]
pub enum ConstGeneric {
    /// A global constant
    Global(GlobalDeclId::Id),
    /// A const generic variable
    Var(ConstGenericVarId::Id),
    /// A concrete value
    Value(Literal),
}

/// A type.
///
/// Types are parameterized by a type parameter used for regions (or lifetimes).
/// The reason is that in MIR, regions are used in the function signatures but
/// are erased in the function bodies. We make this extremely explicit (and less
/// error prone) in our encoding by using two different types: [`Region`](Region)
/// and [`ErasedRegion`](ErasedRegion), the latter being an enumeration with only
/// one variant.
#[derive(
    Debug,
    PartialEq,
    Eq,
    Clone,
    VariantName,
    EnumIsA,
    EnumAsGetters,
    EnumToGetters,
    VariantIndexArity,
    Serialize,
)]
pub enum Ty<R>
where
    R: Clone + std::cmp::Eq, // TODO: do we really need to put those here?
{
    /// An ADT.
    /// Note that here ADTs are very general. They can be:
    /// - user-defined ADTs
    /// - tuples (including `unit`, which is a 0-tuple)
    /// - assumed types (includes some primitive types, e.g., arrays or slices)
    /// The information on the nature of the ADT is stored in (`TypeId`)[TypeId].
    /// The last list is used encode const generics, e.g., the size of an array
    Adt(TypeId, Vec<R>, Vec<Ty<R>>, Vec<ConstGeneric>),
    TypeVar(TypeVarId::Id),
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
    /// TODO: but do we really use this type for variables?...
    Never,
    // We don't support floating point numbers on purpose
    /// A borrow
    Ref(R, Box<Ty<R>>, RefKind),
    /// A raw pointer.
    ///
    /// We need this not only for unsafe code, but also to extract optimized
    /// MIR: in optimized MIR, boxe dereferences and moves out of boxes is
    /// desugared to very low-level code, which manipulates raw pointers
    /// but also `std::ptr::Unique` and `std::ptr::NonNull`. In particular,
    /// if `b` is a `Box<T>`, `x := move *b` is compiled to something like this:
    /// ```text
    /// tmp = (((b.0: std::ptr::Unique<T>).0: std::ptr::NonNull<T>).0: *const T);
    /// x = move (*tmp);
    /// ```
    ///
    /// Also, deallocation leads to the following code (this is independent of the
    /// level of MIR):
    /// ```text
    /// alloc::alloc::box_free::<T, std::alloc::Global>(
    ///     move (b.0: std::ptr::Unique<T>),
    ///     move (b.1: std::alloc::Global))
    /// ```
    /// For now, we detect this case (this is hardcoded in [crate::register] and
    /// [crate::translate_functions_to_ullbc]) to rewrite it to `free(move b)`.
    ///
    /// TODO: maybe we should simply deactivate support for optimized code: who
    /// wants to verify this?
    RawPtr(Box<Ty<R>>, RefKind),
}

/// Type with *R*egions.
///
/// Used in function signatures and type definitions.
/// TODO: rename to sty (*signature* type). Region types are used by the
/// interpreter.
pub type RTy = Ty<Region<RegionVarId::Id>>;

/// Type with *E*rased regions.
///
/// Used in function bodies, "general" value types, etc.
pub type ETy = Ty<ErasedRegion>;

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
#[derive(Debug, PartialEq, Eq, Clone, Copy, EnumIsA, EnumAsGetters, VariantName, Serialize)]
pub enum AssumedTy {
    /// Boxes have a special treatment: we translate them as identity.
    Box,
    /// Comes from the standard library
    Vec,
    /// Comes from the standard library
    Option,
    /// Comes from the standard library
    Range,
    /// Comes from the standard library. See the comments for [Ty::RawPtr]
    /// as to why we have this here.
    PtrUnique,
    /// Same comments as for [AssumedTy::PtrUnique]
    PtrNonNull,
    /// Primitive type
    Array,
    /// Primitive type
    Slice,
    /// Primitive type
    Str,
}
