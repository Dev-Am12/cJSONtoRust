#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NodeId(pub usize);

#[derive(Debug, PartialEq)]
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
