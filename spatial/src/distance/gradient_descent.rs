//! Gradient descent approximation functions for numerical solutions to distance problems.
//!
//! These methods are highly reliable, but also slow, and therefore should primarily be used for accurate comparisons to
//! faster algorithms or optimized code paths in testing, includiing prop testing.
//!
//! Cases covered are:
//! - point-to-line where lines are constant longitude (great circle lines have an analytical solution)
//! - line-to-line where both lines are constant longitude
//!
//! These functions are specifically work on overlapping meridian segments and should not be used in other contexts.

use std::f64::consts::PI;

use geo::{GeoFloat, Line, Point};

use crate::p;

use super::{haversine, MAX_ITERATIONS_MSG, VALID_GF};

const DEBUG_DISPLAY: bool = false;
const TOLERANCE: f64 = 1e-7;

/// Gradient descent method for calculating the minimum distance between a meridian segment and a point.
///
/// This method should be reliable but slow and can be used for testing purposes as a reference implementation for
/// prop testing or to find roots manually for unit tests.
#[allow(unused)]
pub(super) fn merdian_to_point<T: GeoFloat>(
    lat_a: T,
    lat_b: T,
    lon: T,
    point: &Point<T>,
) -> (T, Point<T>) {
    // Setup, including ensuring correct ordering of the input lat range
    // We use a tighter tolerance for accuracy rather than efficiency
    let tolerance = T::from(TOLERANCE).expect(VALID_GF);

    let lat_min = lat_a.min(lat_b);
    let lat_max = lat_a.max(lat_b);

    // In order to account for the cases where there is a local max in the domain, we set our initial T to be the end
    // with the smallest distance, which should guarantee that we don't return a non-minimum endpoint
    let dist_at_min = haversine(&p!(lon, lat_min), point);
    let dist_at_max = haversine(&p!(lon, lat_max), point);

    // Start at the better endpoint for gradient descent
    let mut t = if dist_at_min <= dist_at_max {
        T::zero()
    } else {
        T::one()
    };

    let mut learning_rate = T::from(0.1).expect(VALID_GF);
    let h = T::from(1e-8).expect(VALID_GF); // For gradient computation

    let mut d_prev;
    let mut d_cur = T::from(PI).expect(VALID_GF); // Max possible value as starting point
    let mut pt_cur = p!(lon, lat_min + t * (lat_max - lat_min));

    if DEBUG_DISPLAY {
        eprintln!("   i t_value  d_cur    gradient    lr       | lat");
    }

    for i in 0..=10_000 {
        d_prev = d_cur;

        // Current point and distance
        pt_cur = p!(lon, lat_min + t * (lat_max - lat_min));
        d_cur = haversine(&pt_cur, point);

        // Compute gradient using finite differences
        let t_plus = (t + h).min(T::one());
        let t_minus = (t - h).max(T::zero());

        let pt_plus = p!(lon, lat_min + t_plus * (lat_max - lat_min));
        let pt_minus = p!(lon, lat_min + t_minus * (lat_max - lat_min));

        let d_plus = haversine(&pt_plus, point);
        let d_minus = haversine(&pt_minus, point);

        let gradient = (d_plus - d_minus) / (t_plus - t_minus);
        let gradient_norm = gradient.abs();

        if DEBUG_DISPLAY {
            eprintln!(
                "{:>4} {:.6} {:.6} {:+.8} {:.6} | {:+.10}",
                i,
                t.to_f64().unwrap(),
                d_cur.to_f64().unwrap(),
                gradient.to_f64().unwrap(),
                learning_rate.to_f64().unwrap(),
                pt_cur.y().to_f64().unwrap(),
            );
        }

        // Check convergence on both distance change and gradient magnitude
        if (d_prev - d_cur).abs() < tolerance && gradient_norm < tolerance {
            return (d_cur, pt_cur);
        }

        // Handle boundary cases - if at boundary and gradient points outward, we're done
        if t == T::zero() && gradient >= T::zero() {
            return (d_cur, pt_cur);
        }
        if t == T::one() && gradient <= T::zero() {
            return (d_cur, pt_cur);
        }

        // Adaptive learning rate
        learning_rate = if d_cur > d_prev {
            learning_rate * T::from(0.5).expect(VALID_GF)
        } else {
            learning_rate * T::from(1.1).expect(VALID_GF)
        };

        // Clamp learning rate to reasonable bounds
        learning_rate = learning_rate.clamp(
            T::from(1e-8).expect(VALID_GF),
            T::from(0.5).expect(VALID_GF),
        );

        let base_step_size = T::from(0.01).expect(VALID_GF);
        let step = learning_rate * base_step_size * (gradient / gradient_norm);

        // Gradient descent; clamp to [0, 1]
        let new_t = t - step;
        t = new_t.clamp(T::zero(), T::one());
    }

    // TODO: Detmine whether we do last distance or panic here in production version
    // Safety hatch - looks like a solution won't converge with these inputs
    //panic!("{}", MAX_ITERATIONS_MSG);
    (d_cur, pt_cur)
}

/// Gradient descent method for calculating the minimum distance between two overlapping meridian segments.
///
/// This method should be reliable but slow and can be used for testing purposes as a reference implementation for
/// prop testing or to find roots manually for unit tests.
/// TODO: Make sure this works well and converges all the time
#[allow(unused)]
pub(super) fn meridian_to_meridian<T: GeoFloat>(
    l1: &Line<T>,
    l2: &Line<T>,
) -> (T, Point<T>, Point<T>) {
    let tolerance = T::from(TOLERANCE).expect(VALID_GF);
    let h = T::from(1e-8).expect(VALID_GF);
    let two = T::one() + T::one();
    let mut learning_rate = T::from(0.01).expect(VALID_GF);

    let mut t1 = T::from(0.5).expect(VALID_GF);
    let mut t2 = t1;
    let mut pt1 = l1.start_point();
    let mut pt2 = l2.start_point();
    let mut d = T::from(PI).expect(VALID_GF);

    if DEBUG_DISPLAY {
        eprintln!("   i: t1       t2       | pt1_lat   pt2_lat   | d_cur    lr");
    }

    for i in 0..=10_000 {
        let (pt1_plus, pt1_minus) = extract_points(t1, &l1, h);
        let (pt2_plus, pt2_minus) = extract_points(t2, &l2, h);

        let grad1 = (haversine(&pt1_plus, &pt2) - haversine(&pt1_minus, &pt2)) / (two * h);
        let grad2 = (haversine(&pt2_plus, &pt1) - haversine(&pt2_minus, &pt1)) / (two * h);

        // Apply an adaptive learning rate that scales back movement as it gets closer to convergence, then
        // calculate the new distance
        let t1_new = (t1 - learning_rate * grad1).clamp(T::zero(), T::one());
        let t2_new = (t2 - learning_rate * grad2).clamp(T::zero(), T::one());

        pt1 = p!(l1.start.x, l1.start.y + t1_new * (l1.end.y - l1.start.y));
        pt2 = p!(l2.start.x, l2.start.y + t2_new * (l2.end.y - l2.start.y));
        let d_new = haversine(&pt1, &pt2);

        // Check convergence
        if (d_new - d).abs() < tolerance
            && ((t1_new - t1).powi(2) + (t2_new - t2).powi(2)).sqrt() < tolerance
        {
            return (d, pt1, pt2);
        }

        // Change the learning rate to reflect progress being made, moving further when heading in the right
        // direction, but backing off when getting further from the minimum to slow down the approach
        if d_new < d {
            learning_rate = learning_rate * T::from(1.1).expect(VALID_GF);
        } else {
            learning_rate = learning_rate * T::from(0.5).expect(VALID_GF);
        }

        t1 = t1_new;
        t2 = t2_new;
        d = d_new;

        if DEBUG_DISPLAY {
            eprintln!(
                "{:>4}: {:.6} {:.6} | {:+.6} {:+.6} | {:.6} {:.6}",
                i,
                t1.to_f64().unwrap(),
                t2.to_f64().unwrap(),
                pt1.y().to_f64().unwrap(),
                pt2.y().to_f64().unwrap(),
                d.to_f64().unwrap(),
                learning_rate.to_f64().unwrap(),
            );
        }
    }

    // TODO: Detmine whether we do last distance or panic here in production version
    // Safety hatch - looks like a solution won't converge with these inputs
    //panic!("{}", MAX_ITERATIONS_MSG);
    (d, pt1, pt2)
}

fn extract_points<T: GeoFloat>(t: T, line: &Line<T>, h: T) -> (Point<T>, Point<T>) {
    let lat = line.start.y + t * (line.end.y - line.start.y);

    (p!(line.start.x, lat + h), p!(line.start.x, lat - h))
}

// Some minimal tests to ensure rect-rect is working while Newton's method is still under construction
// All distance values were validated as less than the appropriate value in Excel, but copied from the result here, so
// could be pressure tested further due to Excel's precision issues
#[cfg(test)]
mod tests {
    use super::*;
    use crate::l;
    use approx::assert_abs_diff_eq;

    /// Constant for a single degree in radians
    const DEGREE: f64 = 0.01745;

    const EPSILON: f64 = 1e-6;

    #[test]
    fn same_lat_picks_closest_to_poles() {
        let l1 = l!(0.0, 0.0, 0.0, DEGREE * 20.0);
        let l2 = l!(DEGREE * 10.0, 0.0, DEGREE * 10.0, DEGREE * 20.0);

        let (d, p1, p2) = meridian_to_meridian(&l1, &l2);

        assert_abs_diff_eq!(d, 0.1639559, epsilon = EPSILON);
        assert_abs_diff_eq!(p1.y(), DEGREE * 20.0, epsilon = EPSILON);
        assert_abs_diff_eq!(p2.y(), DEGREE * 20.0, epsilon = EPSILON);

        // Let's also make sure we don't mess up the lon
        assert_abs_diff_eq!(p1.x(), 0.0, epsilon = EPSILON);
        assert_abs_diff_eq!(p2.x(), DEGREE * 10.0, epsilon = EPSILON);
    }

    #[test]
    fn works_across_the_antimeridian() {
        // Also indicates it will translate across any lon change
        let l1 = l!(DEGREE * -5.0, 0.0, DEGREE * -5.0, DEGREE * 20.0);
        let l2 = l!(DEGREE * 5.0, 0.0, DEGREE * 5.0, DEGREE * 20.0);

        let (d, p1, p2) = meridian_to_meridian(&l1, &l2);

        assert_abs_diff_eq!(d, 0.1639559, epsilon = EPSILON);
        assert_abs_diff_eq!(p1.y(), DEGREE * 20.0, epsilon = EPSILON);
        assert_abs_diff_eq!(p2.y(), DEGREE * 20.0, epsilon = EPSILON);
    }

    #[test]
    fn chooses_smaller_boundary_point_and_above_on_larger() {
        let l1 = l!(0.0, 0.0, 0.0, DEGREE * 20.0);
        let l2 = l!(DEGREE * 10.0, 0.0, DEGREE * 10.0, DEGREE * 25.0);

        let (d, p1, p2) = meridian_to_meridian(&l1, &l2);

        assert_abs_diff_eq!(d, 0.1638819, epsilon = EPSILON);
        assert_abs_diff_eq!(p1.y(), DEGREE * 20.0, epsilon = EPSILON);
        assert!(p2.y() > DEGREE * 20.1);
        assert!(p2.y() < DEGREE * 21.0);
    }

    #[test]
    fn works_when_close() {
        let l1 = l!(0.0, 0.0, 0.0, DEGREE * 20.0);
        let l2 = l!(DEGREE, 0.0, DEGREE, DEGREE * 20.0);

        let (d, p1, p2) = meridian_to_meridian(&l1, &l2);

        assert_abs_diff_eq!(d, 0.0163980, epsilon = EPSILON);
        assert_abs_diff_eq!(p1.y(), DEGREE * 20.0, epsilon = EPSILON);
        assert_abs_diff_eq!(p2.y(), DEGREE * 20.0, epsilon = EPSILON);
    }

    #[test]
    fn works_when_not_overlapping_end_to_end() {
        let l1 = l!(0.0, 0.0, 0.0, DEGREE * 5.0);
        let l2 = l!(DEGREE * 5.0, DEGREE * 10.0, DEGREE * 5.0, DEGREE * 20.0);

        let (d, p1, p2) = meridian_to_meridian(&l1, &l2);

        assert_abs_diff_eq!(d, 0.122843, epsilon = EPSILON);
        assert_abs_diff_eq!(p1.y(), DEGREE * 5.0, epsilon = EPSILON);
        assert_abs_diff_eq!(p2.y(), DEGREE * 10.0, epsilon = EPSILON);
    }

    #[test]
    fn works_when_not_overlapping_and_ends_are_close() {
        let l1 = l!(0.0, 0.0, 0.0, DEGREE * 5.0);
        let l2 = l!(DEGREE * 20.0, DEGREE * 5.1, DEGREE * 20.0, DEGREE * 20.0);

        let (d, p1, p2) = meridian_to_meridian(&l1, &l2);

        assert_abs_diff_eq!(d, 0.347616, epsilon = EPSILON);
        assert_abs_diff_eq!(p1.y(), DEGREE * 5.0, epsilon = EPSILON);
        assert!(p2.y() > DEGREE * 5.1);
        assert!(p2.y() < DEGREE * 6.0);
    }

    #[test]
    fn finds_the_min_with_internal_max_northern() {
        let l1 = l!(0.0, DEGREE * -10.0, 0.0, DEGREE * 20.0);
        let l2 = l!(DEGREE * 10.0, DEGREE * -10.0, DEGREE * 10.0, DEGREE * 20.0);

        let (d, p1, p2) = meridian_to_meridian(&l1, &l2);

        assert_abs_diff_eq!(d, 0.163956, epsilon = EPSILON);
        assert_abs_diff_eq!(p1.y(), DEGREE * 20.0, epsilon = EPSILON);
        assert_abs_diff_eq!(p2.y(), DEGREE * 20.0, epsilon = EPSILON);
    }

    #[test]
    fn finds_the_min_with_internal_max_southern() {
        let l1 = l!(0.0, DEGREE * -20.0, 0.0, DEGREE * 10.0);
        let l2 = l!(DEGREE * 10.0, DEGREE * -20.0, DEGREE * 10.0, DEGREE * 10.0);

        let (d, p1, p2) = meridian_to_meridian(&l1, &l2);

        assert_abs_diff_eq!(d, 0.163956, epsilon = EPSILON);
        assert_abs_diff_eq!(p1.y(), DEGREE * -20.0, epsilon = EPSILON);
        assert_abs_diff_eq!(p2.y(), DEGREE * -20.0, epsilon = EPSILON);
    }
}
