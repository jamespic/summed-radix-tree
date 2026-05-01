use pyo3::prelude::*;

/// A Python module implemented in Rust.
#[pymodule]
mod summed_radix_tree {
    use std::{array, sync::{Arc, LazyLock}};

    use pyo3::{exceptions::PyStopIteration, prelude::*, types::{PyDict, PyTuple}};
    use xxhash_rust::xxh3::{Xxh3Default};

    const FANOUT: usize = 8;

    #[pyclass(frozen)]
    struct SummedRadixTree(Arc<InnerSummedRadixTree>);

    #[pymethods]
    impl SummedRadixTree {
        #[new]
        #[pyo3(signature = (items=None))]
        fn new(items: Option<&Bound<'_, PyDict>>) -> PyResult<Self> {
            let mut bitset = EMPTY.clone();
            if let Some(items) = items {
                for (key, value) in items.iter() {
                    let key: usize = key.extract()?;
                    let value: u64 = value.extract()?;
                    bitset = bitset.add(key, value);
                }
            }
            Ok(SummedRadixTree(bitset))
        }

        fn contains(&self, element: usize) -> bool {
            self.0.get_value(element) != 0
        }

        fn __contains__(&self, element: usize) -> bool {
            self.contains(element)
        }

        fn add(&self, element: usize, value: u64) -> Self {
            SummedRadixTree(self.0.add(element, value))
        }

        fn union(&self, other: &SummedRadixTree) -> Self {
            SummedRadixTree(self.0.union(&other.0))
        }

        #[getter]
        fn total(&self) -> u64 {
            self.0.total()
        }

        fn __or__(&self, other: &SummedRadixTree) -> Self {
            self.union(other)
        }

        fn __add__(&self, other: &Bound<'_, PyTuple>) -> PyResult<Self> {
            if other.len() != 2 {
                return Err(PyErr::new::<pyo3::exceptions::PyValueError, _>("Expected a tuple of (element, value)"));
            }
            let element: usize = other.get_item(0)?.extract()?;
            let value: u64 = other.get_item(1)?.extract()?;
            Ok(self.add(element, value))
        }

        fn __getitem__(&self, element: usize) -> u64 {
            self.0.get_value(element)
        }

        fn __hash__(&self) -> u64 {
            self.0.unique_hash() as u64
        }

        fn __eq__(&self, other: &SummedRadixTree) -> bool {
            self.0.unique_hash() == other.0.unique_hash()
        }

        fn __iter__(&self) -> _Iterator {
            _Iterator(SummedRadixTreeIterator::new(self.0.clone(), 0))
        }

        fn __str__(&self) -> String {
            let elements: Vec<String> = self.__iter__().0.map(|e| format!("{}: {}", e.0, e.1)).collect();
            format!("SummedRadixTree({{{}}})", elements.join(", "))
        }

        fn __repr__(&self) -> String {
            self.__str__()
        }

        fn __len__(&self) -> usize {
            self.__iter__().0.count()
        }

        fn __sizeof__(&self) -> usize {
            size_of::<Self>() + self.0._estimate_size_fudging_refcounts()
        }
    }

    #[pyclass]
    struct _Iterator(SummedRadixTreeIterator);

    #[derive(Debug)]
    enum InnerSummedRadixTree {
        Empty,
        Leaf{values:[u64; FANOUT], hash: u128, total: u64},
        Branch {
            level: u8,
            children: [Arc<InnerSummedRadixTree>; FANOUT],
            hash: u128,
            total: u64
        },
    }

    static EMPTY: LazyLock<Arc<InnerSummedRadixTree>> = LazyLock::new(|| Arc::new(InnerSummedRadixTree::Empty));

    impl InnerSummedRadixTree {
        fn get_value(&self, position: usize) -> u64 {
            match self {
                Self::Empty => 0,
                Self::Leaf { values, .. } => {
                    if position >= FANOUT {
                        0
                    } else {
                        values[position]
                    }
                }
                Self::Branch { level, children, .. } => {
                    let items_per_element = FANOUT.pow(*level as u32);
                    let child_index = position / items_per_element;
                    if child_index >= FANOUT {
                        0
                    } else {
                        let child_offset = position % items_per_element;
                        children[child_index].get_value(child_offset)
                        
                    }
                }
            }
        }

        fn unique_hash(&self) -> u128 {
            match self {
                Self::Empty => 0,
                Self::Leaf { hash, .. } => *hash,
                Self::Branch { hash, .. } => *hash,
            }
        }

        fn total(&self) -> u64 {
            match self {
                Self::Empty => 0,
                Self::Leaf { total, .. } => *total,
                Self::Branch { total, .. } => *total,
            }
        }

        fn add(self: &Arc<Self>, position: usize, value: u64) -> Arc<Self> {
            if self.get_value(position) == value {
                self.clone()
            } else {
                let single_position_set = Self::_with_single_position_set(position, value);
                self.union(&Arc::new(single_position_set))
            }
        }

        fn union<'a>(self: &Arc<Self>, other: &Arc<Self>) -> Arc<Self> {
            match (self.as_ref(), other.as_ref()) {
                (Self::Empty, _) => other.clone(),
                (_, Self::Empty) => self.clone(),
                (s, o) if s.unique_hash() == o.unique_hash() => self.clone(),
                (Self::Leaf { .. }, Self::Branch { .. }) => {
                    other.union(self)  // Let the bigger tree handle merging
                }
                (Self::Branch { level, children, .. }, Self::Leaf { .. }) => {
                    let mut new_children = children.clone();
                    new_children[0] = new_children[0].clone().union(other);
                    let hash = Self::_calculate_branch_hash(&new_children);
                    let total = new_children.iter().map(|child| child.total()).sum();
                    Arc::new(Self::Branch {
                        level: *level,
                        children: new_children,
                        hash,
                        total,
                    })
                }
                (Self::Branch { level: l1, children: c1, .. }, Self::Branch { level: l2, children: c2, .. }) => {
                    match l1.cmp(l2) {
                        std::cmp::Ordering::Greater => {
                            let mut new_children = c1.clone();
                            new_children[0] = new_children[0].clone().union(other);
                            let hash = Self::_calculate_branch_hash(&new_children);
                            let total = new_children.iter().map(|child| child.total()).sum();
                            Arc::new(Self::Branch {
                                level: *l1,
                                children: new_children,
                                hash,
                                total,
                            })
                        }
                        std::cmp::Ordering::Less => {
                            other.union(self)  // Let the bigger tree handle merging
                        }
                        std::cmp::Ordering::Equal => {
                            let new_children = array::from_fn(|i| c1[i].clone().union(&c2[i]));
                            let hash = Self::_calculate_branch_hash(&new_children);
                            let total = new_children.iter().map(|child| child.total()).sum();
                            Arc::new(Self::Branch {
                                level: *l1,
                                children: new_children,
                                hash,
                                total,
                            })
                        }
                    }
                }
                (Self::Leaf { values: v1, .. }, Self::Leaf { values: v2, .. }) => {
                    if v1 == v2 {
                        self.clone()
                    } else {
                        let new_values = array::from_fn(|i| v1[i].max(v2[i]));
                        let hash = Self::_calculate_leaf_hash(&new_values);
                        let total = new_values.iter().sum();
                        Arc::new(Self::Leaf {
                            values: new_values,
                            hash,
                            total,
                        })
                    }
                }
                
                
            }
        }


        fn _with_single_position_set(position: usize, value: u64) -> Self {
            let leaf_index = position / FANOUT;
            let leaf_position = position % FANOUT;

            let mut leaf_values = [0u64; FANOUT];
            leaf_values[leaf_position] = value;
            let mut result = Self::Leaf {
                values: leaf_values,
                hash: Self::_calculate_leaf_hash(&leaf_values),
                total: value
            };

            // Make parent branch nodes as needed
            let mut current_level = 1;
            let mut child_index = leaf_index;
            while child_index > 0 {
                let parent_index = child_index % FANOUT;
                child_index /= FANOUT;
                let mut children: [Arc<Self>; FANOUT] = array::from_fn(|_| EMPTY.clone());
                children[parent_index] = Arc::new(result);
                let hash = Self::_calculate_branch_hash(&children);
                let new_branch = Self::Branch {
                    level: current_level,
                    children,
                    hash,
                    total: value,
                };
                result = new_branch;
                current_level += 1;
            }

            result
            
        }

        fn _calculate_branch_hash(children: &[Arc<Self>; FANOUT]) -> u128 {
            let mut hasher = Xxh3Default::new();
            for child in children {
                let child_hash = child.unique_hash();
                hasher.update(&child_hash.to_le_bytes());
            }
            hasher.digest128()
        }

        fn _calculate_leaf_hash(values: &[u64; FANOUT]) -> u128 {
            let mut hasher = Xxh3Default::new();
            for bit in values {
                hasher.update(&bit.to_le_bytes());
            }
            hasher.digest128()
        }

        fn _estimate_size(&self) -> usize {
            match self {
                Self::Empty => 0,
                Self::Leaf { .. } => size_of::<Self>(),
                Self::Branch { children, .. } => {
                    size_of::<Self>()
                    + children.iter().map(
                        |child| child._estimate_size_fudging_refcounts()
                    ).sum::<usize>()
                }
            }
        }

        fn _estimate_size_fudging_refcounts(self: &Arc<Self>) -> usize {
            return (self._estimate_size() / Arc::strong_count(&self)) + size_of::<Arc<()>>();
        }
    }

    struct SummedRadixTreeIterator {
        tree: Arc<InnerSummedRadixTree>,
        offset: usize,
        child_index: usize,
        child_iter: Option<Box<SummedRadixTreeIterator>>,
    }

    impl SummedRadixTreeIterator {
        fn new(tree: Arc<InnerSummedRadixTree>, offset: usize) -> Self {
            Self {
                tree,
                offset,
                child_index: 0,
                child_iter: None,
            }
        }
    }

    impl Iterator for SummedRadixTreeIterator {
        type Item = (usize, u64);

        fn next(&mut self) -> Option<Self::Item> {
            match self.tree.as_ref() {
                InnerSummedRadixTree::Empty => None,
                InnerSummedRadixTree::Leaf { values, .. } => {
                    while self.child_index < FANOUT {
                        let value = values[self.child_index];
                        let current_offset = self.offset + self.child_index;
                        self.child_index += 1;
                        if value > 0 {
                            return Some((current_offset, value));
                        }
                    }
                    None
                }
                InnerSummedRadixTree::Branch { level, children, .. } => {
                    if let Some(child_iter) = &mut self.child_iter {
                        if let Some(item) = child_iter.next() {
                            return Some(item);
                        } else {
                            self.child_iter = None; // Finished with this child
                        }
                    }

                    while self.child_index < FANOUT {
                        let child = &children[self.child_index];
                        let current_offset = self.offset + self.child_index * FANOUT.pow((*level) as u32);
                        self.child_index += 1;
                        if child.total() > 0 {
                            self.child_iter = Some(Box::new(SummedRadixTreeIterator::new(child.clone(), current_offset)));
                            if let Some(item) = self.child_iter.as_mut().unwrap().next() {
                                return Some(item);
                            } else {
                                self.child_iter = None; // Finished with this child
                            }
                        }
                    }
                    None
                }
            }
        }
    }

    #[pymethods]
    impl _Iterator {
        fn __iter__(slf: PyRef<Self>) -> PyRef<Self> {
            slf
        }

        fn __next__(mut slf: PyRefMut<Self>) -> PyResult<(usize, u64)> {
            slf.0.next().ok_or_else(|| PyStopIteration::new_err("No more items"))
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn test_basic_operations() {
            let tree = SummedRadixTree::new(None).unwrap();
            assert!(!tree.contains(5));
            let tree = tree.add(5, 10);
            assert!(tree.contains(5));
            assert_eq!(tree.total(), 10);

            let tree2 = SummedRadixTree::new(None).unwrap().add(5, 15).add(10, 20);
            let union_tree = tree.union(&tree2);
            assert!(union_tree.contains(5));
            assert!(union_tree.contains(10));
            assert_eq!(union_tree.total(), 35);
        }

        fn compare_with_naive_implementation(values: Vec<Vec<(usize, u64)>>) {
            let mut naive_map = std::collections::HashMap::new();
            let mut tree = SummedRadixTree::new(None).unwrap();

            for subset in values {
                let mut inner_tree = SummedRadixTree::new(None).unwrap();
                for (element, value) in subset {
                    inner_tree = inner_tree.add(element, value);
                    naive_map.insert(element, value);
                }
                tree = tree.union(&inner_tree);
            }

            for (element, value) in &naive_map {
                assert!(tree.contains(*element));
                assert_eq!(tree.0.get_value(*element), *value);
            }
            assert!(tree.total() == naive_map.values().sum::<u64>());
        }

        macro_rules! generate_tests {
            ($($name:ident: $values:expr),+) => {
                $(
                    #[test]
                    fn $name() {
                        compare_with_naive_implementation($values);
                    }
                )*
            };
        }

        generate_tests!(
            test_single_element: vec![vec![(5, 10)]],
            test_multiple_elements: vec![vec![(5, 10), (10, 20), (15, 30)]],
            test_overlapping_elements: vec![vec![(5, 10)], vec![(5, 15)], vec![(10, 20)]],
            test_large_numbers: vec![vec![(1000, 1), (2000, 2)], vec![(1000, 3), (3000, 4)]],
            test_empty_tree: vec![],
            test_powers_of_two: vec![vec![(0, 1), (8, 2), (64, 3), (512, 4)]],
            test_descending_powers_of_two: vec![vec![(512, 4), (64, 3), (8, 2), (0, 1)]],
            test_one_to_ten: vec![(0..10).map(|i| (i, i as u64 + 1)).collect()],
            test_descending_one_to_ten: vec![(0..10).rev().map(|i| (i, i as u64 + 1)).collect()]
        );


    }



}
