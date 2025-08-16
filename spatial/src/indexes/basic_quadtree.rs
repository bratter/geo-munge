//! Basic starting point quadtree implementation.

use std::{
    ops::Deref,
    sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard},
};

use anyhow::{anyhow, bail, Result};
use geo::{Geometry, Rect};

use super::{ProximitySearch, RegionQuery, SpatialIndex};

use crate::{
    math::{rect_in_rect, Bbox},
    Distance,
};

/// The number of records to store in a given node before subdividing.
///
/// Balances the linear lookup cost of this fixed list against node depth. While it addes fixed overhead, it helps
/// amortize the cost of unbalanced trees.
const MAX_CHILDREN: usize = 8;

/// Maximum subdivision depth to prevent infinite recursion with duplicate geometries.
///
/// When geometries have identical bounding boxes, subdivision will repeatedly place them in the same
/// quadrant. This depth limit ensures we eventually stop subdividing and store duplicates as stuck_children.
///
/// TODO: Always bottoming out the max depth rather than stopping at a higher level may lead to unecessary work in many
/// cases, but as this is likely going to be used as a reference implementation, likely just leave it.
const MAX_DEPTH: usize = 20;

/// Expect message for a poisoned lock.
const POISON: &str = "Lock poisoned";

/// Reference implementation of a quadtree as a starting point for index development.
pub struct BasicQuadTree<T> {
    root: Arc<RwLock<Node<T>>>,
}

impl<T> BasicQuadTree<T> {
    pub fn new(bbox: Rect) -> Self {
        Self {
            root: Arc::new(RwLock::new(Node::new(bbox, 0))),
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

    // TODO: Change this implementation to something more efficient
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

    // TODO: See note above regarding O(n) implementation
    fn contains<K>(&self, id: &K) -> bool
    where
        T::Target: PartialEq<K>,
    {
        for node_lock in self.iter_nodes() {
            let node = node_lock.read().expect(POISON);

            if node.stuck_children.iter().any(|c| c.deref() == id) {
                return true;
            }
            if node.children.iter().any(|c| c.deref() == id) {
                return true;
            }
        }

        false
    }
}

#[derive(Debug)]
struct Node<T> {
    bbox: Rect,
    depth: usize,
    nodes: Option<[Arc<RwLock<Node<T>>>; 4]>,
    children: Vec<T>,
    stuck_children: Vec<T>,
}

impl<T> Node<T> {
    fn new(bbox: Rect, depth: usize) -> Self {
        Self {
            bbox,
            depth,
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
            // When the children array is filled, we subdivide the node (unless we've reached max depth)
            None if self.children.len() >= MAX_CHILDREN && self.depth < MAX_DEPTH => {
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
        let child_depth = self.depth + 1;

        self.nodes = Some([
            Arc::new(RwLock::new(Self::new(tl, child_depth))),
            Arc::new(RwLock::new(Self::new(tr, child_depth))),
            Arc::new(RwLock::new(Self::new(br, child_depth))),
            Arc::new(RwLock::new(Self::new(bl, child_depth))),
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

/// Iterator for nearest neighbor search with radius constraint.
pub struct NearestIterator<'a, T> {
    cmp: &'a Geometry,
    r: f64,
    work: Vec<(WorkType<T>, f64)>,
}

impl<'a, T> NearestIterator<'a, T>
where
    T: Deref + Clone,
    T::Target: AsRef<Geometry>,
{
    fn new(root: Arc<RwLock<Node<T>>>, cmp: &'a Geometry, r: f64) -> Self {
        let d_root = root.read().expect(POISON).bbox.distance(cmp);
        let work = vec![(WorkType::Node(root), d_root)];

        Self { cmp, r, work }
    }
}

impl<'a, T> Iterator for NearestIterator<'a, T>
where
    T: Deref + Clone,
    T::Target: AsRef<Geometry>,
{
    type Item = (T, f64);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            // Sort the work stack to process closest elements first
            self.work.sort_by(|(_, d1), (_, d2)| {
                d2.partial_cmp(d1)
                    .expect("Invalid distance already removed")
            });

            // Process children first (they're actual results)
            while let Some(&(WorkType::Child(ref child), d)) = self.work.last() {
                // Stop if distance exceeds radius
                if d > self.r {
                    return None;
                }

                let result = (child.clone(), d);
                self.work.pop();
                return Some(result);
            }

            // Process nodes (expand them into children and sub-nodes)
            if let Some((WorkType::Node(node), d)) = self.work.pop() {
                // Stop if distance exceeds radius
                if d > self.r {
                    return None;
                }

                let node = node.read().expect(POISON);

                // Add all children to work stack
                for child in node.stuck_children.iter().chain(&node.children) {
                    let d: f64 = self.cmp.distance(child.as_ref());

                    if d.is_finite() {
                        self.work.push((WorkType::Child(child.clone()), d));
                    }
                }

                // Add sub-nodes to work stack
                if let Some(nodes) = &node.nodes {
                    for sub_node in nodes {
                        let bbox = sub_node.read().expect(POISON).bbox;
                        let d: f64 = bbox.distance(self.cmp);

                        if d.is_finite() {
                            self.work.push((WorkType::Node(Arc::clone(&sub_node)), d));
                        }
                    }
                }
            } else {
                // No more work to do
                return None;
            }
        }
    }
}

impl<T> ProximitySearch<T> for BasicQuadTree<T>
where
    T: Deref + Clone,
    T::Target: AsRef<Geometry>,
{
    fn within_radius(&self, cmp: &Geometry, radius: f64) -> impl Iterator<Item = (T, f64)> {
        NearestIterator::new(Arc::clone(&self.root), cmp, radius)
    }
}

// WARN: Placeholder implementation only
impl<'a, T: 'a> RegionQuery<'a, T> for BasicQuadTree<T> {
    fn contained_by(&'a self, _bbox: &Rect) -> impl Iterator<Item = &'a T> {
        std::iter::empty()
    }

    fn intersecting(&'a self, _bbox: &Rect) -> impl Iterator<Item = &'a T> {
        std::iter::empty()
    }
}

#[cfg(test)]
mod test {
    use approx::assert_abs_diff_eq;

    use super::*;

    use crate::{
        harness::{read_cities_as_record, read_city_pairs, TestRecord},
        math::get_earth_bbox,
        p, MEAN_EARTH_RADIUS,
    };

    #[test]
    fn knn_returns_self_d_equals_0_london() {
        let name = "London";
        let cities = read_cities_as_record();
        let cmp = cities
            .iter()
            .find(|c| c.name == name)
            .unwrap()
            .point
            .clone();

        let qt = BasicQuadTree::new(get_earth_bbox());
        for city in &cities {
            qt.insert(city).unwrap();
        }

        let (record, d) = qt.closest(&cmp).unwrap();
        assert_eq!(record.name, name);
        assert_eq!(d, 0.0);
    }

    // Note that this was failing to converge, so checking specifically.
    #[test]
    fn knn_returns_self_d_equals_0_sydney() {
        let name = "Sydney";
        let cities = read_cities_as_record();
        let cmp = cities
            .iter()
            .find(|c| c.name == name)
            .unwrap()
            .point
            .clone();

        let qt = BasicQuadTree::new(get_earth_bbox());
        for city in &cities {
            qt.insert(city).unwrap();
        }

        let (record, d) = qt.closest(&cmp).unwrap();
        assert_eq!(record.name, name);
        assert_eq!(d, 0.0);
    }

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

        let knn_result = qt.neighbors(&cmp).collect::<Vec<_>>();

        // We return all records
        assert_eq!(knn_result.len(), cities.len());

        // We are in the right order and right distances
        // Tested to the nearest meter (approx)
        for ((test, test_d), (exp_name, exp_d)) in knn_result.iter().zip(city_dist) {
            assert_eq!(test.name, exp_name);
            assert_abs_diff_eq!(*test_d * MEAN_EARTH_RADIUS, exp_d, epsilon = 1.0);
        }
    }

    // Ensure that the data structure doesn't overlfow the stack when adding mulitple points at the same location by
    // infinitely recursing during subdivision
    // We also check that we return only k matches, even though more exist at the same point
    #[test]
    fn does_not_overflow_with_points_at_same_location() {
        let qt = BasicQuadTree::new(get_earth_bbox());
        let point = p!(0.1, 0.2);

        for i in 0..20 {
            let record = TestRecord {
                name: i.to_string(),
                point: geo::Geometry::Point(point.clone()),
            };
            qt.insert(record).unwrap();
        }

        let test = geo::Geometry::Point(p!(0.0, 0.0));
        let res: Vec<_> = qt.nearest(&test, 3).collect();

        assert_eq!(res.len(), 3);
        assert_eq!(res[0].0.point, res[1].0.point);
        assert_eq!(res[0].0.point, res[2].0.point);
    }
}
