#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NodeId(pub usize);

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NodeType {
    Null,
    False,
    True,
    Number,
    String,
    Array,
    Object,
    Raw,
}

pub struct Node {
    pub next: Option<NodeId>,
    pub prev: Option<NodeId>,
    pub child: Option<NodeId>,
    pub node_type: NodeType,
    pub value_string: Option<Vec<u8>>,
    pub value_double: f64,
    pub key: Option<Vec<u8>>,
    pub is_reference: bool,
    pub key_is_const: bool,
}

pub struct Arena {
    nodes: Vec<Node>,
    deleted: Vec<bool>,
}

pub const CJSON_CIRCULAR_LIMIT: usize = 10_000;

impl Default for Arena {
    fn default() -> Self {
        Self::new()
    }
}

impl Arena {
    pub fn new() -> Self {
        Self {
            nodes: Vec::new(),
            deleted: Vec::new(),
        }
    }

    pub fn alloc(&mut self, node: Node) -> NodeId {
        let id = NodeId(self.nodes.len());
        self.nodes.push(node);
        self.deleted.push(false);
        id
    }

    pub fn get(&self, id: NodeId) -> &Node {
        &self.nodes[id.0]
    }

    pub fn get_mut(&mut self, id: NodeId) -> &mut Node {
        &mut self.nodes[id.0]
    }

    pub fn is_deleted(&self, id: NodeId) -> bool {
        self.deleted[id.0]
    }

    pub fn delete(&mut self, id: NodeId) {
        self.delete_chain(Some(id));
    }

    fn delete_chain(&mut self, mut current: Option<NodeId>) {
        while let Some(id) = current {
            if self.is_deleted(id) {
                return;
            }

            let (next, child, is_reference, key_is_const) = {
                let node = self.get(id);
                (node.next, node.child, node.is_reference, node.key_is_const)
            };

            if !is_reference {
                self.delete_chain(child);
            }

            let node = self.get_mut(id);
            node.next = None;
            node.prev = None;
            node.child = None;
            if !is_reference {
                node.value_string = None;
                if !key_is_const {
                    node.key = None;
                }
            }
            self.deleted[id.0] = true;
            current = next;
        }
    }

    fn is_live_node(&self, id: NodeId) -> bool {
        id.0 < self.nodes.len() && !self.deleted[id.0]
    }

    pub fn print_number(&self, id: NodeId) -> Option<String> {
        if !self.is_live_node(id) {
            return None;
        }

        let value = self.get(id).value_double;
        if value.is_nan() || value.is_infinite() {
            return Some("null".to_string());
        }

        let integer_value = if value >= i32::MAX as f64 {
            i32::MAX
        } else if value <= i32::MIN as f64 {
            i32::MIN
        } else {
            value as i32
        };
        if value == integer_value as f64 {
            return Some(integer_value.to_string());
        }

        let first_attempt = Self::format_g(value, 15);
        if Self::round_trips_with_cjson_epsilon(&first_attempt, value) {
            return Some(first_attempt);
        }

        Some(Self::format_g(value, 17))
    }

    fn format_g(value: f64, precision: usize) -> String {
        let exponent = Self::decimal_exponent(value);
        let mut rendered = if (-4..precision as i32).contains(&exponent) {
            let decimal_places = (precision as i32 - (exponent + 1)).max(0) as usize;
            format!("{:.*}", decimal_places, value)
        } else {
            format!("{:.*e}", precision - 1, value)
        };

        if let Some(exponent_marker) = rendered.find('e') {
            let mantissa = Self::strip_fraction_zeros(&rendered[..exponent_marker]);
            let exponent: i32 = rendered[exponent_marker + 1..]
                .parse()
                .expect("Rust float formatting produces a valid exponent");
            rendered = format!(
                "{}e{}{:02}",
                mantissa,
                if exponent < 0 { '-' } else { '+' },
                exponent.abs()
            );
        } else {
            rendered = Self::strip_fraction_zeros(&rendered);
        }
        rendered
    }

    fn decimal_exponent(value: f64) -> i32 {
        let magnitude = value.abs();
        let mut exponent = magnitude.log10().floor() as i32;
        while magnitude < 10_f64.powi(exponent) {
            exponent -= 1;
        }
        while magnitude >= 10_f64.powi(exponent + 1) {
            exponent += 1;
        }
        exponent
    }

    fn strip_fraction_zeros(value: &str) -> String {
        let Some(decimal_point) = value.find('.') else {
            return value.to_string();
        };
        let mut end = value.len();
        while end > decimal_point + 1 && value.as_bytes()[end - 1] == b'0' {
            end -= 1;
        }
        if end == decimal_point + 1 {
            end -= 1;
        }
        value[..end].to_string()
    }

    fn round_trips_with_cjson_epsilon(rendered: &str, original: f64) -> bool {
        let Ok(parsed) = rendered.parse::<f64>() else {
            return false;
        };
        let max_value = parsed.abs().max(original.abs());
        (parsed - original).abs() <= max_value * f64::EPSILON
    }

    pub fn compare(&self, a: NodeId, b: NodeId, case_sensitive: bool) -> bool {
        if !self.is_live_node(a) || !self.is_live_node(b) {
            return false;
        }
        self.compare_nodes(a, b, case_sensitive)
    }

    fn compare_nodes(&self, a: NodeId, b: NodeId, case_sensitive: bool) -> bool {
        let a_node = self.get(a);
        let b_node = self.get(b);
        if a_node.node_type != b_node.node_type {
            return false;
        }
        if a == b {
            return true;
        }

        match a_node.node_type {
            NodeType::Null | NodeType::False | NodeType::True => true,
            NodeType::Number => Self::compare_numbers(a_node.value_double, b_node.value_double),
            NodeType::String | NodeType::Raw => {
                match (&a_node.value_string, &b_node.value_string) {
                    (Some(a_value), Some(b_value)) => a_value == b_value,
                    _ => false,
                }
            }
            NodeType::Array => self.compare_arrays(a_node.child, b_node.child, case_sensitive),
            NodeType::Object => self.compare_objects(a, b, case_sensitive),
        }
    }

    fn compare_numbers(a: f64, b: f64) -> bool {
        let max_value = a.abs().max(b.abs());
        (a - b).abs() <= max_value * f64::EPSILON
    }

    fn compare_arrays(
        &self,
        mut a_element: Option<NodeId>,
        mut b_element: Option<NodeId>,
        case_sensitive: bool,
    ) -> bool {
        loop {
            match (a_element, b_element) {
                (Some(a), Some(b)) => {
                    if !self.compare_nodes(a, b, case_sensitive) {
                        return false;
                    }
                    a_element = self.get(a).next;
                    b_element = self.get(b).next;
                }
                (None, None) => return true,
                _ => return false,
            }
        }
    }

    fn compare_objects(&self, a: NodeId, b: NodeId, case_sensitive: bool) -> bool {
        self.object_members_match(a, b, case_sensitive)
            && self.object_members_match(b, a, case_sensitive)
    }

    fn object_members_match(&self, source: NodeId, target: NodeId, case_sensitive: bool) -> bool {
        let mut source_element = self.get(source).child;
        while let Some(element) = source_element {
            let node = self.get(element);
            let Some(key) = node.key.as_deref() else {
                return false;
            };
            let Some(match_in_target) = self.find_object_child(target, key, case_sensitive) else {
                return false;
            };
            if !self.compare_nodes(element, match_in_target, case_sensitive) {
                return false;
            }
            source_element = node.next;
        }
        true
    }

    pub fn duplicate(&mut self, source: NodeId, recurse: bool) -> Option<NodeId> {
        if !self.is_live_node(source) {
            return None;
        }

        let root = self.duplicate_single_node(source);
        let mut duplicated_nodes = vec![root];
        if !recurse {
            return Some(root);
        }

        let mut pending = vec![(source, root, 0_usize)];
        while let Some((source_parent, duplicate_parent, depth)) = pending.pop() {
            let mut source_child = self.get(source_parent).child;
            let mut previous_duplicate_child = None;

            while let Some(child) = source_child {
                if depth >= CJSON_CIRCULAR_LIMIT || !self.is_live_node(child) {
                    self.discard_nodes(&duplicated_nodes);
                    return None;
                }

                let source_next = self.get(child).next;
                let duplicate_child = self.duplicate_single_node(child);
                duplicated_nodes.push(duplicate_child);

                match previous_duplicate_child {
                    Some(previous) => self.get_mut(previous).next = Some(duplicate_child),
                    None => self.get_mut(duplicate_parent).child = Some(duplicate_child),
                }
                self.get_mut(duplicate_child).prev = previous_duplicate_child;

                pending.push((child, duplicate_child, depth + 1));
                previous_duplicate_child = Some(duplicate_child);
                source_child = source_next;
            }
        }

        Some(root)
    }

    fn duplicate_single_node(&mut self, source: NodeId) -> NodeId {
        let node = self.get(source);
        self.alloc(Node {
            next: None,
            prev: None,
            child: None,
            node_type: node.node_type,
            value_string: node.value_string.clone(),
            value_double: node.value_double,
            key: node.key.clone(),
            is_reference: false,
            key_is_const: false,
        })
    }

    fn discard_nodes(&mut self, nodes: &[NodeId]) {
        for &id in nodes {
            let node = self.get_mut(id);
            node.next = None;
            node.prev = None;
            node.child = None;
            node.value_string = None;
            node.key = None;
            self.deleted[id.0] = true;
        }
    }

    fn alloc_simple(
        &mut self,
        node_type: NodeType,
        value_string: Option<Vec<u8>>,
        value_double: f64,
    ) -> NodeId {
        self.alloc(Node {
            next: None,
            prev: None,
            child: None,
            node_type,
            value_string,
            value_double,
            key: None,
            is_reference: false,
            key_is_const: false,
        })
    }

    pub fn create_null(&mut self) -> NodeId {
        self.alloc_simple(NodeType::Null, None, 0.0)
    }

    pub fn create_true(&mut self) -> NodeId {
        self.alloc_simple(NodeType::True, None, 0.0)
    }

    pub fn create_false(&mut self) -> NodeId {
        self.alloc_simple(NodeType::False, None, 0.0)
    }

    pub fn create_bool(&mut self, value: bool) -> NodeId {
        if value {
            self.create_true()
        } else {
            self.create_false()
        }
    }

    pub fn create_number(&mut self, value: f64) -> NodeId {
        self.alloc_simple(NodeType::Number, None, value)
    }

    pub fn create_string(&mut self, value: Vec<u8>) -> NodeId {
        self.alloc_simple(NodeType::String, Some(value), 0.0)
    }

    pub fn create_raw(&mut self, value: Vec<u8>) -> NodeId {
        self.alloc_simple(NodeType::Raw, Some(value), 0.0)
    }

    pub fn create_string_reference(&mut self, value: Vec<u8>) -> NodeId {
        let id = self.alloc_simple(NodeType::String, Some(value), 0.0);
        self.get_mut(id).is_reference = true;
        id
    }

    pub fn create_array(&mut self) -> NodeId {
        self.alloc_simple(NodeType::Array, None, 0.0)
    }

    pub fn create_object(&mut self) -> NodeId {
        self.alloc_simple(NodeType::Object, None, 0.0)
    }

    pub fn create_object_reference(&mut self, source: NodeId) -> NodeId {
        let child = self.get(source).child;
        self.alloc(Node {
            next: None,
            prev: None,
            child,
            node_type: NodeType::Object,
            value_string: None,
            value_double: 0.0,
            key: None,
            is_reference: true,
            key_is_const: false,
        })
    }

    pub fn create_array_reference(&mut self, source: NodeId) -> NodeId {
        let child = self.get(source).child;
        self.alloc(Node {
            next: None,
            prev: None,
            child,
            node_type: NodeType::Array,
            value_string: None,
            value_double: 0.0,
            key: None,
            is_reference: true,
            key_is_const: false,
        })
    }

    pub fn add_item_to_array(&mut self, array: NodeId, item: NodeId) -> bool {
        if array == item || !self.is_live_node(array) || !self.is_live_node(item) {
            return false;
        }

        self.append_child(array, item, None);
        true
    }

    pub fn add_item_to_object(&mut self, object: NodeId, key: Vec<u8>, item: NodeId) -> bool {
        if object == item || !self.is_live_node(object) || !self.is_live_node(item) {
            return false;
        }

        self.append_child(object, item, Some(key));
        true
    }

    fn add_created_to_object(
        &mut self,
        object: NodeId,
        key: Vec<u8>,
        item: NodeId,
    ) -> Option<NodeId> {
        if self.add_item_to_object(object, key, item) {
            Some(item)
        } else {
            self.delete(item);
            None
        }
    }

    pub fn add_null_to_object(&mut self, object: NodeId, key: Vec<u8>) -> Option<NodeId> {
        let item = self.create_null();
        self.add_created_to_object(object, key, item)
    }

    pub fn add_true_to_object(&mut self, object: NodeId, key: Vec<u8>) -> Option<NodeId> {
        let item = self.create_true();
        self.add_created_to_object(object, key, item)
    }

    pub fn add_false_to_object(&mut self, object: NodeId, key: Vec<u8>) -> Option<NodeId> {
        let item = self.create_false();
        self.add_created_to_object(object, key, item)
    }

    pub fn add_bool_to_object(
        &mut self,
        object: NodeId,
        key: Vec<u8>,
        value: bool,
    ) -> Option<NodeId> {
        let item = self.create_bool(value);
        self.add_created_to_object(object, key, item)
    }

    pub fn add_number_to_object(
        &mut self,
        object: NodeId,
        key: Vec<u8>,
        value: f64,
    ) -> Option<NodeId> {
        let item = self.create_number(value);
        self.add_created_to_object(object, key, item)
    }

    pub fn add_string_to_object(
        &mut self,
        object: NodeId,
        key: Vec<u8>,
        value: Vec<u8>,
    ) -> Option<NodeId> {
        let item = self.create_string(value);
        self.add_created_to_object(object, key, item)
    }

    pub fn add_raw_to_object(
        &mut self,
        object: NodeId,
        key: Vec<u8>,
        value: Vec<u8>,
    ) -> Option<NodeId> {
        let item = self.create_raw(value);
        self.add_created_to_object(object, key, item)
    }

    pub fn detach_item_via_pointer(&mut self, parent: NodeId, item: NodeId) -> Option<NodeId> {
        if !self.is_live_node(parent) || !self.is_live_node(item) {
            return None;
        }

        let mut previous = None;
        let mut current = self.get(parent).child;
        while let Some(id) = current {
            if id == item {
                break;
            }
            previous = Some(id);
            current = self.get(id).next;
        }

        if current != Some(item) {
            return None;
        }

        let next = self.get(item).next;
        match previous {
            Some(previous) => self.get_mut(previous).next = next,
            None => self.get_mut(parent).child = next,
        }
        if let Some(next) = next {
            self.get_mut(next).prev = previous;
        }

        self.reset_attachment(item, None);
        Some(item)
    }

    pub fn detach_item_from_array(&mut self, array: NodeId, index: usize) -> Option<NodeId> {
        if !self.is_live_node(array) {
            return None;
        }

        let mut current = self.get(array).child;
        for _ in 0..index {
            current = current.and_then(|id| self.get(id).next);
        }
        self.detach_item_via_pointer(array, current?)
    }

    pub fn detach_item_from_object(&mut self, object: NodeId, key: &[u8]) -> Option<NodeId> {
        let item = self.find_object_child(object, key, false)?;
        self.detach_item_via_pointer(object, item)
    }

    pub fn detach_item_from_object_case_sensitive(
        &mut self,
        object: NodeId,
        key: &[u8],
    ) -> Option<NodeId> {
        let item = self.find_object_child(object, key, true)?;
        self.detach_item_via_pointer(object, item)
    }

    pub fn delete_item_from_array(&mut self, array: NodeId, index: usize) -> bool {
        let Some(item) = self.detach_item_from_array(array, index) else {
            return false;
        };
        self.delete(item);
        true
    }

    pub fn delete_item_from_object(&mut self, object: NodeId, key: &[u8]) -> bool {
        let Some(item) = self.detach_item_from_object(object, key) else {
            return false;
        };
        self.delete(item);
        true
    }

    pub fn delete_item_from_object_case_sensitive(&mut self, object: NodeId, key: &[u8]) -> bool {
        let Some(item) = self.detach_item_from_object_case_sensitive(object, key) else {
            return false;
        };
        self.delete(item);
        true
    }

    pub fn insert_item_in_array(&mut self, array: NodeId, index: usize, item: NodeId) -> bool {
        if array == item || !self.is_live_node(array) || !self.is_live_node(item) {
            return false;
        }

        let mut inserted_before = self.get(array).child;
        for _ in 0..index {
            inserted_before = inserted_before.and_then(|id| self.get(id).next);
        }

        let Some(inserted_before) = inserted_before else {
            return self.add_item_to_array(array, item);
        };
        let previous = self.get(inserted_before).prev;

        self.reset_attachment(item, None);
        self.get_mut(item).next = Some(inserted_before);
        self.get_mut(item).prev = previous;
        self.get_mut(inserted_before).prev = Some(item);
        match previous {
            Some(previous) => self.get_mut(previous).next = Some(item),
            None => self.get_mut(array).child = Some(item),
        }
        true
    }

    pub fn replace_item_via_pointer(
        &mut self,
        parent: NodeId,
        old_item: NodeId,
        new_item: NodeId,
    ) -> bool {
        if parent == new_item
            || !self.is_live_node(parent)
            || !self.is_live_node(old_item)
            || !self.is_live_node(new_item)
        {
            return false;
        }

        let mut previous = None;
        let mut current = self.get(parent).child;
        while let Some(id) = current {
            if id == old_item {
                break;
            }
            previous = Some(id);
            current = self.get(id).next;
        }
        if current != Some(old_item) {
            return false;
        }

        if old_item == new_item {
            return true;
        }

        let next = self.get(old_item).next;
        self.get_mut(new_item).next = next;
        self.get_mut(new_item).prev = previous;

        if let Some(next) = next {
            self.get_mut(next).prev = Some(new_item);
        }
        match previous {
            Some(previous) => self.get_mut(previous).next = Some(new_item),
            None => self.get_mut(parent).child = Some(new_item),
        }

        self.get_mut(old_item).next = None;
        self.get_mut(old_item).prev = None;
        self.delete(old_item);
        true
    }

    pub fn replace_item_in_array(&mut self, array: NodeId, index: usize, new_item: NodeId) -> bool {
        if !self.is_live_node(array) || !self.is_live_node(new_item) {
            return false;
        }

        let mut old_item = self.get(array).child;
        for _ in 0..index {
            old_item = old_item.and_then(|id| self.get(id).next);
        }
        let Some(old_item) = old_item else {
            return false;
        };
        self.replace_item_via_pointer(array, old_item, new_item)
    }

    pub fn replace_item_in_object(&mut self, object: NodeId, key: &[u8], new_item: NodeId) -> bool {
        self.replace_item_in_object_by_case(object, key, new_item, false)
    }

    pub fn replace_item_in_object_case_sensitive(
        &mut self,
        object: NodeId,
        key: &[u8],
        new_item: NodeId,
    ) -> bool {
        self.replace_item_in_object_by_case(object, key, new_item, true)
    }

    fn replace_item_in_object_by_case(
        &mut self,
        object: NodeId,
        key: &[u8],
        new_item: NodeId,
        case_sensitive: bool,
    ) -> bool {
        if !self.is_live_node(object) || !self.is_live_node(new_item) {
            return false;
        }

        self.get_mut(new_item).key = Some(key.to_vec());
        self.get_mut(new_item).key_is_const = false;
        let Some(old_item) = self.find_object_child(object, key, case_sensitive) else {
            return false;
        };
        self.replace_item_via_pointer(object, old_item, new_item)
    }

    fn find_object_child(
        &self,
        object: NodeId,
        key: &[u8],
        case_sensitive: bool,
    ) -> Option<NodeId> {
        if !self.is_live_node(object) {
            return None;
        }

        let mut current = self.get(object).child;
        while let Some(id) = current {
            let node = self.get(id);
            let matches = node.key.as_deref().is_some_and(|candidate| {
                if case_sensitive {
                    candidate == key
                } else {
                    candidate.eq_ignore_ascii_case(key)
                }
            });
            if matches {
                return Some(id);
            }
            current = node.next;
        }

        None
    }

    fn reset_attachment(&mut self, item: NodeId, key: Option<Vec<u8>>) {
        let node = self.get_mut(item);
        node.next = None;
        node.prev = None;
        node.key = key;
    }

    pub fn append_child(&mut self, parent: NodeId, child: NodeId, key: Option<Vec<u8>>) {
        assert_ne!(parent, child, "a node cannot be its own child");

        self.reset_attachment(child, key);

        let first_child = self.get(parent).child;
        match first_child {
            None => self.get_mut(parent).child = Some(child),
            Some(mut last_child) => {
                while let Some(next) = self.get(last_child).next {
                    last_child = next;
                }

                self.get_mut(last_child).next = Some(child);
                self.get_mut(child).prev = Some(last_child);
            }
        }
    }
}
