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
}

impl Default for Arena {
    fn default() -> Self {
        Self::new()
    }
}

impl Arena {
    pub fn new() -> Self {
        Self { nodes: Vec::new() }
    }

    pub fn alloc(&mut self, node: Node) -> NodeId {
        let id = NodeId(self.nodes.len());
        self.nodes.push(node);
        id
    }

    pub fn get(&self, id: NodeId) -> &Node {
        &self.nodes[id.0]
    }

    pub fn get_mut(&mut self, id: NodeId) -> &mut Node {
        &mut self.nodes[id.0]
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
}
