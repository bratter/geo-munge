//! Basic starting point quadtree implementation.

use std::{
    ops::Deref,
    sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard},
};

use anyhow::{anyhow, bail, Result};
use geo::{Geometry, Rect};

use super::{BboxSearch, Knn, SpatialIndex};

use crate::{
    math::{rect_in_rect, Bbox},
    Distance,
};

/// The number of records to store in a given node before subdividing.
///
/// Balances the linear lookup cost of this fixed list against node depth. While it addes fixed overhead, it helps
/// amortize the cost of unbalanced trees.
const MAX_CHILDREN: usize = 8;

const POISON: &str = "Lock poisoned";

/// Simple reference implementation of a quadtree as a starting point for index development.
pub struct BasicQuadTree<T> {
    root: Arc<RwLock<Node<T>>>,
}

impl<T> BasicQuadTree<T> {
    pub fn new(bbox: Rect) -> Self {
        Self {
            root: Arc::new(RwLock::new(Node::new(bbox))),
        }
    }

    fn read(&self) -> RwLockReadGuard<'_, Node<T>> {
        self.root.read().expect(POISON)
    }

    fn write(&self) -> RwLockWriteGuard<'_, Node<T>> {
        self.root.write().expect(POISON)
    }

    fn iter_nodes(&self) -> NodeIter<T> {
        NodeIter::new(Arc::clone(&self.root))
    }
}

impl<T> SpatialIndex<T> for BasicQuadTree<T>
where
    T: Deref,
    T::Target: AsRef<Geometry>,
{
    fn insert(&self, record: T) -> Result<()> {
        let g: &Geometry = record.as_ref();
        let bbox = g
            .bbox()
            .ok_or(anyhow!("Cannot make bounding rect for geometry"))?;

        if !rect_in_rect(&self.read().bbox, &bbox) {
            bail!("Geometry not within the index bbox");
        }

        self.write().insert(record, &bbox);

        Ok(())
    }

    // TODO: Change this implementation to something more efficienct
    // This is currently O(n), but possibly no need to change if the whole implementation here is just going to be used
    // as a reference impl.
    fn remove<K>(&self, id: &K) -> Option<T>
    where
        T::Target: PartialEq<K>,
    {
        for node_lock in self.iter_nodes() {
            // Take the write guard anyway rather than doing multiple passes
            let mut node = node_lock.write().expect(POISON);

            if let Some(pos) = node.stuck_children.iter().position(|c| c.deref() == id) {
                return Some(node.stuck_children.swap_remove(pos));
            }
            if let Some(pos) = node.children.iter().position(|c| c.deref() == id) {
                return Some(node.children.swap_remove(pos));
            }
        }
        None
    }
}

struct Node<T> {
    bbox: Rect,
    nodes: Option<[Arc<RwLock<Node<T>>>; 4]>,
    children: Vec<T>,
    stuck_children: Vec<T>,
}

impl<T> Node<T> {
    fn new(bbox: Rect) -> Self {
        Self {
            bbox,
            nodes: None,
            children: Vec::new(),
            stuck_children: Vec::new(),
        }
    }
}

impl<T> Node<T>
where
    T: Deref,
    T::Target: AsRef<Geometry>,
{
    fn insert(&mut self, record: T, bbox: &Rect) {
        match self.nodes.take() {
            // When there are already sub-nodes, we try to push further into the tree, however if this may block if a
            // geometry crosses multiple sub-nodes, at which point it is added to stuck_children
            Some(nodes) => {
                let node_idx = self.node_index(&bbox);
                let mut sub_node = nodes[node_idx as usize].write().expect(POISON);

                if rect_in_rect(&sub_node.bbox, bbox) {
                    sub_node.insert(record, bbox);
                } else {
                    self.stuck_children.push(record);
                }

                // Make sure to replace the nodes
                drop(sub_node);
                self.nodes = Some(nodes);
            }
            // When the children array is filled, we subdivide the node
            None if self.children.len() >= MAX_CHILDREN => {
                self.subdivide();

                // Recurse to re-insert the child nodes. Could do inline here, but recursion overhead will be minimal
                for child in std::mem::take(&mut self.children) {
                    let bbox = child.as_ref().bbox_unchecked();

                    self.insert(child, &bbox);
                }

                self.insert(record, bbox);
            }
            // Otherwise can just push the new element
            None => self.children.push(record),
        }
    }

    fn subdivide(&mut self) {
        let [l, r] = self.bbox.split_x();
        let [tl, bl] = l.split_y();
        let [tr, br] = r.split_y();

        self.nodes = Some([
            Arc::new(RwLock::new(Self::new(tl))),
            Arc::new(RwLock::new(Self::new(tr))),
            Arc::new(RwLock::new(Self::new(br))),
            Arc::new(RwLock::new(Self::new(bl))),
        ]);
    }

    fn node_index(&self, bbox: &Rect) -> NodeIndex {
        let (cx, cy) = self.bbox.center().x_y();
        let (x, y) = bbox.min().x_y();

        match (x < cx, y < cy) {
            (true, true) => NodeIndex::TopLeft,
            (false, true) => NodeIndex::TopRight,
            (false, false) => NodeIndex::BottomRight,
            (true, false) => NodeIndex::BottomLeft,
        }
    }
}

// TODO: If we usually use preorder, it might be slightly better to reverse the order here
// to enable extension of preorder stacks
enum NodeIndex {
    TopLeft = 0,
    TopRight = 1,
    BottomRight = 2,
    BottomLeft = 3,
}

struct NodeIter<T> {
    stack: Vec<Arc<RwLock<Node<T>>>>,
}

impl<T> NodeIter<T> {
    fn new(root: Arc<RwLock<Node<T>>>) -> Self {
        Self { stack: vec![root] }
    }
}

impl<T> Iterator for NodeIter<T> {
    type Item = Arc<RwLock<Node<T>>>;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(current) = self.stack.pop() {
            if let Some(nodes) = &current.read().expect(POISON).nodes {
                self.stack.push(Arc::clone(&nodes[3]));
                self.stack.push(Arc::clone(&nodes[2]));
                self.stack.push(Arc::clone(&nodes[1]));
                self.stack.push(Arc::clone(&nodes[0]));
            };

            Some(current)
        } else {
            None
        }
    }
}

enum WorkType<T> {
    Child(T),
    Node(Arc<RwLock<Node<T>>>),
}

// TODO: What behavior if multiple nodes at same point?
// TODO: Explicitly determine and document error behavior in the trait
impl<T> Knn<T> for BasicQuadTree<T>
where
    T: Deref + Clone,
    T::Target: AsRef<Geometry>,
{
    fn knn_r(&self, cmp: &Geometry, k: usize, r: f64) -> impl Iterator<Item = (T, f64)> {
        // Start by seeding the work stack with the root node
        // TODO: Do we allow errors if the cmp is not within the root? Don't think it matters, but should check. If we
        // do, then it makes it more appropriate to error elsewhere
        let root = Arc::clone(&self.root);
        let d_root = root.read().expect(POISON).bbox.distance(cmp);

        let mut work: Vec<(WorkType<T>, f64)> = vec![(WorkType::Node(root), d_root)];
        let mut results = Vec::new();

        // Traverse the work stack in distance sorted order
        loop {
            // Sort the elements in the work stack as we need to operate on the closest elements first
            // Pop must get the closest element, so need to sort descending
            work.sort_by(|(_, d1), (_, d2)| {
                d2.partial_cmp(d1)
                    .expect("Invalid distance already removed")
            });

            // Iterate through all children in an inner loop - avoid unnecessary re-sorting when nothing additional has
            // been added
            while let Some(&(WorkType::Child(ref child), d)) = work.last() {
                // As soon as our distance exceeds the threshold, we are done
                if d > r {
                    return results.into_iter();
                }

                // Now push the results until we reach k results
                results.push((child.clone(), d));

                // Pop inside the while loop as we need to iterate before popping, but we don't need the pop result
                work.pop();

                // TODO: How to handle/document equal distances... think make it arbitrary so that len never exceeds k
                if results.len() >= k {
                    return results.into_iter();
                }
            }

            // When a node is within radius, push its children and sub-nodes onto the work stack
            // Radius comparison doesn't happen on insertion, only when checked
            if let Some((WorkType::Node(node), d)) = work.pop() {
                if d > r {
                    return results.into_iter();
                }

                let node = node.read().expect(POISON);

                for child in node.stuck_children.iter().chain(&node.children) {
                    let d: f64 = cmp.distance(child.as_ref());

                    // Only add children where the distance is not NaN or infinite
                    // TODO: Confirm that we just skip invalids here, i.e., be forgiving
                    if d.is_finite() {
                        work.push((WorkType::Child(child.clone()), d));
                    }
                }

                if let Some(nodes) = &node.nodes {
                    for sub_node in nodes {
                        let bbox = sub_node.read().expect(POISON).bbox;
                        let d: f64 = bbox.distance(cmp);

                        // Only push sub nodes where the distance is not NaN or infinite
                        // TODO: This should never be the case, so can delete?
                        if d.is_finite() {
                            work.push((WorkType::Node(Arc::clone(&sub_node)), d));
                        }
                    }
                }
            } else {
                return results.into_iter();
            }
        }
    }
}

// WARN: Placeholder implementation only
impl<'a, T: 'a> BboxSearch<'a, T> for BasicQuadTree<T> {
    fn get_bbox(&self, _bbox: &Rect) -> impl Iterator<Item = &T> {
        std::iter::empty()
    }
}

#[cfg(test)]
mod test {
    use super::*;

    use crate::{
        harness::{read_cities_as_record, read_city_pairs},
        math::get_earth_bbox,
    };

    #[test]
    fn knn_iterates_all_in_order() {
        let mut city_dist = read_city_pairs()
            .into_iter()
            .filter_map(|((a, b), d)| if a == "London" { Some((b, d)) } else { None })
            .collect::<Vec<_>>();
        city_dist.sort_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap());

        let cities = read_cities_as_record();
        let cmp = cities
            .iter()
            .find(|c| c.name == "London")
            .unwrap()
            .point
            .clone();

        let qt = BasicQuadTree::new(get_earth_bbox());
        for city in &cities {
            qt.insert(city).unwrap();
        }

        let knn_result = qt.knn(&cmp, usize::MAX).collect::<Vec<_>>();

        eprintln!("{}", knn_result.len());
        eprintln!("{}", knn_result[0].0.name);
        eprintln!("{}", knn_result[1].0.name);
    }
}
