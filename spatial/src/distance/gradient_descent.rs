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

use geo::{GeoFloat, Line, Point};

use crate::p;

use super::{haversine, MAX_ITERATIONS_MSG, VALID_GF};

const DEBUG_DISPLAY: bool = false;
const TOLERANCE: f64 = 1e-8;

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

    // For gradient descent, start at t = 0
    let mut t = T::zero();
    let mut delta = T::from(0.1).expect(VALID_GF);
    let mut d_prev: T;
    let mut d_cur = T::from(9.0).expect(VALID_GF); // As long as it's bigger than PI it should be fine

    if DEBUG_DISPLAY {
        eprintln!("   i t_value  d_prev   d_cur    | lat");
    }

    for i in 0..=1000 {
        d_prev = d_cur;

        let pt_cur = p!(lon, lat_min + t * (lat_max - lat_min));
        d_cur = haversine(&pt_cur, point);

        if DEBUG_DISPLAY {
            eprintln!(
                "{:>4} {:.6} {:.6} {:.6} | {:+.6}",
                i,
                t.to_f64().unwrap(),
                d_prev.to_f64().unwrap(),
                d_cur.to_f64().unwrap(),
                pt_cur.y().to_f64().unwrap(),
            );
        }

        if (d_prev - d_cur).abs() < tolerance {
            return (d_cur, pt_cur);
        }

        // Halve the delta if we blow past the minimum point
        if d_cur > d_prev {
            delta = -delta / (T::one() + T::one());
        }

        // t must be clamped between 0 and 1, but when the delta is large, we don't want to assume that the minimum
        // can't be between the previous point and the end point, so we back t away from the end point, but more
        // aggressively cut the delta to converge faster as it is quite likely the point is at the end
        let new_t = t + delta;

        if new_t >= T::one() {
            let p_one = p!(lon, lat_max);
            let d_one = haversine(&p_one, point);
            let d_tol = haversine(&p!(lon, lat_max - tolerance), point);

            if d_one < d_tol {
                return (d_one, p_one);
            } else {
                delta = -delta / T::from(4.0).expect(VALID_GF);
            }
        } else if new_t <= T::zero() {
            let p_zero = p!(lon, lat_min);
            let d_zero = haversine(&p_zero, point);
            let d_tol = haversine(&p!(lon, lat_min + tolerance), point);

            if d_zero < d_tol {
                return (d_zero, p_zero);
            } else {
                delta = -delta / T::from(4.0).expect(VALID_GF);
            }
        };

        t = t + delta;
    }

    // Safety hatch - looks like a solution won't converge with these inputs
    panic!("{}", MAX_ITERATIONS_MSG);
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
    let mut d = T::from(9.0).expect(VALID_GF); // As long as it's bigger than PI it should be fine

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

    // Safety hatch - looks like a solution won't converge with these inputs
    panic!("{}", MAX_ITERATIONS_MSG);
}

fn extract_points<T: GeoFloat>(t: T, line: &Line<T>, h: T) -> (Point<T>, Point<T>) {
    let lat = line.start.y + t * (line.end.y - line.start.y);

    (p!(line.start.x, lat + h), p!(line.start.x, lat - h))
}
