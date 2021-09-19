use std::panic::RefUnwindSafe;

use rhizome::sync::Node;
use static_assertions::assert_impl_all;

assert_impl_all!(Node: RefUnwindSafe);
