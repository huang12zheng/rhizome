//! A threading-compatible implementation.

use crate::UnwrapInfallible;
use core::any::TypeId;
use pinus::{prelude::*, sync::PressedPineMap};
use std::{any::Any, borrow::Borrow, pin::Pin, sync::Arc};
use tap::Pipe;

#[cfg(feature = "macros")]
pub use crate::TypeKey;

/// Extension methods for [`Node`] and [`Arc<Node>`].
pub mod extensions {
	use super::{Arc, Node, TypeId};
	use pinus::{prelude::*, sync::PressedPineMap};

	/// Provides the [`.branch_for`](`BranchFor::branch_for`) and [`.branch_for`](`BranchFor::into_branch_for`) methods.
	pub trait BranchFor<Value, Key: Ord, Tag> {
		/// Creates a new [`Node`] instance referencing the current one.
		fn branch_for(&self, tag: Tag) -> Node<Value, Key, Tag>;
		/// Creates a new [`Node`] instance referencing the current one, without cloning this [`Arc`].
		fn into_branch_for(self, tag: Tag) -> Node<Value, Key, Tag>;
	}
	impl<Value, Key: Ord, Tag> BranchFor<Value, Key, Tag> for Arc<Node<Value, Key, Tag>> {
		fn branch_for(&self, tag: Tag) -> Node<Value, Key, Tag> {
			Arc::clone(self).into_branch_for(tag)
		}
		fn into_branch_for(self, tag: Tag) -> Node<Value, Key, Tag> {
			Node {
				parent: Some(self),
				tag,
				local_scope: PressedPineMap::new().pin(),
			}
		}
	}

	/// Provides the [`.branch_with_type_tag`](`BranchWithTypeTag::branch_with_type_tag`) and [`.into_branch_for_type`](`BranchWithTypeTag::into_branch_with_type_tag`) methods.
	pub trait BranchWithTypeTag<Value, Key: Ord>: BranchFor<Value, Key, TypeId> {
		/// Creates a new [`Node`] instance referencing the current one, tagged with the [`TypeId`] of `Tag`.
		fn branch_with_type_tag<Tag: 'static>(&self) -> Node<Value, Key, TypeId>;
		/// Creates a new [`Node`] instance referencing the current one, tagged with the [`TypeId`] of `Tag`, without cloning this [`Arc`].
		fn into_branch_with_type_tag<Tag: 'static>(self) -> Node<Value, Key, TypeId>;
	}
	impl<Value, Key: Ord> BranchWithTypeTag<Value, Key> for Arc<Node<Value, Key, TypeId>> {
		#[inline]
		fn branch_with_type_tag<Tag: 'static>(&self) -> Node<Value, Key, TypeId> {
			self.branch_for(TypeId::of::<Tag>())
		}
		#[inline]
		fn into_branch_with_type_tag<Tag: 'static>(self) -> Node<Value, Key, TypeId> {
			self.into_branch_for(TypeId::of::<Tag>())
		}
	}
}

/// A thread-safe tagged inverse tree node.
pub struct Node<Value: ?Sized = dyn Any, Key: Ord = TypeId, Tag = TypeId> {
	parent: Option<Arc<Node<Value, Key, Tag>>>,
	tag: Tag,
	local_scope: Pin<PressedPineMap<Key, Value>>,
}

impl<Value: ?Sized, Key: Ord, Tag> Node<Value, Key, Tag> {
	/// Creates a new root-[`Node`] with tag `tag`.
	#[must_use]
	pub fn new(tag: Tag) -> Self {
		Self {
			parent: None,
			tag,
			local_scope: PressedPineMap::default().pin(),
		}
	}

	/// Stores `value` for `key` at the current [`Node`].
	///
	/// # Errors
	///
	/// Iff the local scope already contains a value for `key`.
	#[allow(clippy::missing_panics_doc)] //TODO: Validate the panics are indeed unreachable, then clean up potential panic sites.
	pub fn provide(&self, key: Key, value: Value) -> Result<Pin<&Value>, (Key, Value)>
	where
		Value: Sized,
	{
		self.local_scope.insert(key, value)
	}

	/// Stores `value` for `key` at the current [`Node`].
	///
	/// A bit more efficient than [`Self::provide`].
	///
	/// # Errors
	///
	/// Iff the local scope already contains a value for `key`.
	#[allow(clippy::missing_panics_doc)] //TODO: Validate the panics are indeed unreachable, then clean up potential panic sites.
	pub fn provide_mut(&mut self, key: Key, value: Value) -> Result<Pin<&mut Value>, (Key, Value)>
	where
		Value: Sized,
	{
		self.local_scope.insert_mut(key, value)
	}

	/// Wraps this instance in an [`Arc`].
	pub fn into_arc(self) -> Arc<Self> {
		Arc::new(self)
	}

	/// Retrieves a reference to this [`Node`]'s parent [`Node`], if available.
	pub fn parent(&self) -> Option<&Self> {
		self.parent.as_deref()
	}

	/// Retrieves a reference to this [`Node`]'s root [`Node`], which may be itself.
	pub fn root(&self) -> &Self {
		let mut this = self;
		while let Some(parent) = this.parent() {
			this = parent;
		}
		this
	}

	/// Retrieves a reference to the neared [`Node`] node tagged with `tag`.
	///
	/// This may be the current node or any of its ancestors, but no siblings at any level.
	#[must_use]
	pub fn tagged<Q>(&self, tag: &Q) -> Option<&Self>
	where
		Tag: Borrow<Q>,
		Q: PartialEq + ?Sized,
	{
		let mut this = self;
		loop {
			if this.tag.borrow() == tag {
				break Some(this);
			}
			this = this.parent()?;
		}
	}
}

impl<Value, Key: Ord> Node<Value, Key, TypeId> {
	/// Creates a new root-[`Node`] tagged with `Tag`'s [`TypeId`].
	///
	/// # Tracing
	///
	/// ## `error`
	#[must_use]
	pub fn new_with_type_tag<Tag: 'static>() -> Self {
		if tracing::Level::ERROR > tracing::level_filters::STATIC_MAX_LEVEL {}

		Self::new(TypeId::of::<Tag>())
	}

	/// Retrieves a reference to the neared [`Node`] node tagged with `Tag`'s [`TypeId`].
	///
	/// This may be the current node or any of its ancestors, but no siblings at any level.
	#[must_use]
	pub fn type_tagged<Tag: 'static>(&self) -> Option<&Self> {
		self.tagged(&TypeId::of::<Tag>())
	}
}

//TODO: Set up Error enum. (with variants for missing keys and other errors).
//TRACKING: K must be clone until entry_insert/https://github.com/rust-lang/rust/issues/65225 lands.
impl<Value, Key: Ord, Tag> Node<Value, Key, Tag> {
	/// Extracts a value from the [`Node`] tree according to the given `key`.
	#[must_use]
	pub fn get<Q>(&self, key: &Q) -> Option<Pin<&Value>>
	where
		Key: Borrow<Q>,
		Q: Ord + ?Sized,
	{
		self.try_find(|node| node.local_scope.get(key).pipe(Ok))
			.unwrap_infallible()
	}

	/// Extracts a value from this [`Node`] only according to the given `key`.
	///
	/// # Panics
	///
	/// Iff this [`Node`] is poisoned.
	#[must_use]
	pub fn get_local<Q>(&self, key: &Q) -> Option<Pin<&Value>>
	where
		Key: Borrow<Q>,
		Q: Ord + ?Sized,
	{
		self.local_scope.get(key)
	}

	/// Extracts a value from the node tree according to the given `selector`.
	///
	/// The selector is called once each for this [`Node`] and its ancestors, in order of inverse age, until either:
	///
	/// - It returns [`Some`].
	/// - No further parent [`Node`] is available.
	///
	/// # Panics
	///
	/// Not directly, but if a poisoned [`Node`] is reached, then `selector` is likely to panic.
	#[must_use]
	pub fn find<S: Fn(&Node<Value, Key, Tag>) -> Option<Pin<&Value>>>(
		&self,
		local_selector: S,
	) -> Option<Pin<&Value>> {
		self.try_find(|node| Ok(local_selector(node)))
			.unwrap_infallible()
	}

	/// Extracts a value from the node tree according to the given `selector`.
	///
	/// The selector is called once each for this [`Node`] and its ancestors, in order of inverse age, until either:
	///
	/// - It fails.
	/// - It returns [`Some`].
	/// - No further parent [`Node`] is available.
	///
	/// # Errors
	///
	/// Iff the selector fails.
	///
	/// # Panics
	///
	/// Not directly, but if a poisoned [`Node`] is reached, then `selector` is likely to panic.
	pub fn try_find<S: Fn(&Node<Value, Key, Tag>) -> Result<Option<Pin<&Value>>, E>, E>(
		&self,
		local_selector: S,
	) -> Result<Option<Pin<&Value>>, E> {
		let mut this = self;
		loop {
			if let found @ Some(_) = local_selector(this)? {
				break found;
			} else if let Some(parent) = this.parent() {
				this = parent
			} else {
				break None;
			}
		}
		.pipe(Ok)
	}

	//TODO: `search`, `try_search`
}
