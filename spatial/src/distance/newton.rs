//! Newton's method approximation function for numerical solutions.
//!
//! Numerical approximation is required when there is no analytical solution to nearest points. This module contains
//! implementations of Newton's method for simple approximation methodologies.

use geo::{GeoFloat, Line, Point};

use crate::math::{closest, is_between};

use super::haversine::haversine;

const VALID_GF: &str = "Valid GeoFloat";
const MAX_ITERATIONS_MSG: &str = "Max iterations exceeded without convergence";
const DEBUG_DISPLAY: bool = false;
// TODO: Consider switching to scale aware tolerance and delta
const TOLERANCE: f64 = 1e-6;
const DELTA: f64 = 1e-7;

#[inline]
fn tolerance<T: GeoFloat>() -> T {
    T::from(TOLERANCE).expect(VALID_GF)
}

#[inline]
fn delta<T: GeoFloat>() -> T {
    T::from(DELTA).expect(VALID_GF)
}

/// Find the minimum distance from a point to a vertical line segment (constant longitude) using Newton's method
/// optimization.
///
/// # Arguments
/// * `lat_a` - Minimum latitude of the line segment (in radians)
/// * `lat_b` - Maximum latitude of the line segment (in radians)  
/// * `lon` - Constant longitude of the line segment (in radians)
/// * `point` - Target point to find distance to
///
/// # Returns
/// Minimum distance in radians and the final matched point
///
/// # Assumptions
/// - point is inside the latitude bounds of the line represented by `lat_a` and `lat_b`
/// - point is not on the line segment
pub(super) fn vertical_line_to_point<T: GeoFloat>(
    lat_a: T,
    lat_b: T,
    lon: T,
    point: &Point<T>,
) -> (T, Point<T>) {
    const MAX_ITERATIONS: usize = 10;

    // Setup, including ensuring correct ordering of the input lat range
    let tolerance = tolerance();
    let delta = delta();
    let two = T::one() + T::one();
    let lat_min = lat_a.min(lat_b);
    let lat_max = lat_a.max(lat_b);

    // This function should only be used when the point is inside the lat bounds of the line
    debug_assert!(point.y() <= lat_max && point.y() >= lat_min);

    // Constrain the latitude domain for calculation - this works as the function has a well defined relationship where
    // the great circle distance will be closest within a reasonable range of the point
    // TODO: Confirm that this works, potentially using prop testing
    let lon_range = T::from(3.0).expect(VALID_GF) * (point.x() - lon).abs();
    let lat_min = lat_min.max(point.y() - lon_range);
    let lat_max = lat_max.min(point.y() + lon_range);

    // Initial guess: midpoint parameter
    let mut t = T::from(0.5).expect(VALID_GF);

    for i in 0..MAX_ITERATIONS {
        // Current guess of point on line
        let current_lat = lat_min + t * (lat_max - lat_min);
        let current_point = Point::new(lon, current_lat);
        let distance = haversine(&current_point, point);

        // Numerical derivatives using central difference
        // Central difference is more accurate but slightly more calculations than one-sided stepping
        let t_plus = (t + delta).min(T::one());
        let t_minus = (t - delta).max(T::zero());

        let lat_plus = lat_min + t_plus * (lat_max - lat_min);
        let lat_minus = lat_min + t_minus * (lat_max - lat_min);

        let point_plus = Point::new(lon, lat_plus);
        let point_minus = Point::new(lon, lat_minus);

        let dist_plus = haversine(&point_plus, point);
        let dist_minus = haversine(&point_minus, point);

        let first_derivative = (dist_plus - dist_minus) / (two * delta);
        let second_derivative = (dist_plus - two * distance + dist_minus) / (delta * delta);

        // Newton's method update: t_new = t - f'(t) / f''(t)
        // TODO: Confirm that the use of gradient descent here works, maybe with prop testing
        let t_new = if second_derivative.abs() < tolerance {
            // Use gradient descent if second derivative is near zero
            t - T::from(0.01).expect(VALID_GF) * first_derivative
        } else {
            t - first_derivative / second_derivative
        };

        // Clamp to [0, 1] bounds
        let t_clamped = t_new.clamp(T::zero(), T::one());

        if DEBUG_DISPLAY {
            eprintln!(
                "{} {:.6} {:.6} {:.10} {:.10} | {:.10} {:.10}",
                i,
                distance.to_f64().unwrap(),
                current_lat.to_f64().unwrap(),
                t.to_f64().unwrap(),
                t_new.to_f64().unwrap(),
                first_derivative.to_f64().unwrap(),
                second_derivative.to_f64().unwrap(),
            );
        }

        // Check convergence then do a final distance calculation
        if (t_clamped - t).abs() < tolerance {
            // Final distance calculation
            let final_lat = lat_min + t * (lat_max - lat_min);
            let final_point = Point::new(lon, final_lat);

            return (haversine(&final_point, point), final_point);
        }

        t = t_clamped;
    }

    // TODO: Consider if we want to failover gracefully in production
    panic!("{}", MAX_ITERATIONS_MSG);
}

// FIX: This isn't working right - the alternation isn't producing the desired results
pub(super) fn vertical_line_to_line<T: GeoFloat>(l1: &Line<T>, l2: &Line<T>) -> T {
    const MAX_ITERATIONS: usize = 10;

    // Assert verticality
    debug_assert_eq!(l1.dx(), T::zero());
    debug_assert_eq!(l2.dx(), T::zero());

    let tolerance = tolerance::<T>() * T::from(0.1).unwrap();
    let two = T::one() + T::one();

    // The initial point into the vertical line calculation is the midpoint of the second line
    let mut d1;
    let mut d2 = T::infinity();
    let mut p1;
    let mut p2 = Point::new(l2.start.x, (l2.start.y + l2.end.y) / two);

    if DEBUG_DISPLAY {
        eprintln!(
            "l1 {:.4} {:.4} l2 {:.4} {:.4}",
            l1.start.y.to_f64().unwrap(),
            l1.end.y.to_f64().unwrap(),
            l2.start.y.to_f64().unwrap(),
            l2.end.y.to_f64().unwrap()
        );
    }

    for i in 0..MAX_ITERATIONS {
        // First run an optimization using the guess point on the second line, shortcutting if the guess point is
        // outside the range of the y's
        (d1, p1) = inner_vline_to_point(&p2, l1);

        if DEBUG_DISPLAY {
            eprintln!(
                "d1 {} {:.8} | {:.6} {:.6}",
                i,
                d1.to_f64().unwrap(),
                p1.y().to_f64().unwrap(),
                p2.y().to_f64().unwrap()
            );
        }

        if (d1 - d2).abs() < tolerance {
            return d1;
        }

        // Then in the same loop iteration run an optimization using the first line guess point
        (d2, p2) = inner_vline_to_point(&p1, l2);

        if DEBUG_DISPLAY {
            eprintln!(
                "d2 {} {:.8} | {:.6} {:.6}",
                i,
                d2.to_f64().unwrap(),
                p1.y().to_f64().unwrap(),
                p2.y().to_f64().unwrap()
            );
        }

        if (d1 - d2).abs() < tolerance {
            return d2;
        }
    }

    // TODO: Consider if we want to failover gracefully in production
    panic!("{}", MAX_ITERATIONS_MSG);
}

/// Run a single optimization for a given guess point on the second line.
///
/// Manages the special case where the point is outside the range of y's.
fn inner_vline_to_point<T: GeoFloat>(pt: &Point<T>, line: &Line<T>) -> (T, Point<T>) {
    if is_between(pt.y(), line.start.y, line.end.y) {
        vertical_line_to_point(line.start.y, line.end.y, line.start.x, &pt)
    } else {
        let test_pt = Point::new(line.start.x, closest(pt.y(), line.start.y, line.end.y));
        (haversine(&pt, &test_pt), test_pt)
    }
}

#[cfg(test)]
mod tests {
    use crate::p;

    use super::*;
    use approx::assert_abs_diff_eq;
    use std::f64::consts::PI;

    /// Whether to print elp messages in debug numerical solvers.
    const DEBUG_DISPLAY_TEST: bool = false;

    /// Constant for a single degree.
    const DEGREE: f64 = 0.01745;

    /// Subdivision-based fallback method for determining pt - line distances.
    ///
    /// This method should be reliable but slow and can be used for testing purposes as a reference implementation for
    /// prop testing or to find roots manually for unit tests.
    pub(super) fn gradient_descent_vline_pt<T: GeoFloat>(
        lat_a: T,
        lat_b: T,
        lon: T,
        point: &Point<T>,
    ) -> (T, Point<T>) {
        // Setup, including ensuring correct ordering of the input lat range
        // We use a tighter tolerance for accuracy rather than efficiency
        let tolerance = tolerance::<T>() * T::from(0.01).unwrap();
        let lat_min = lat_a.min(lat_b);
        let lat_max = lat_a.max(lat_b);

        // For gradient descent, start at t = 0
        let mut t = T::zero();
        let mut delta = T::from(0.1).unwrap();
        let mut d_cur = T::from(9.0).unwrap(); // As long as it's bigger than PI it should be fine
        let mut d_prev: T;
        let mut pt_cur;

        if DEBUG_DISPLAY_TEST {
            eprintln!("   i t_value  d_prev   d_cur");
        }

        for i in 0..=1000 {
            d_prev = d_cur;

            let current_lat = lat_min + t * (lat_max - lat_min);
            pt_cur = Point::new(lon, current_lat);
            d_cur = haversine(&pt_cur, point);

            if (d_prev - d_cur).abs() < tolerance {
                return (d_cur, pt_cur);
            }

            if d_cur > d_prev {
                delta = -delta / (T::one() + T::one());
            }

            if DEBUG_DISPLAY_TEST {
                eprintln!(
                    "{:>4} {:.6} {:.6} {:.6}",
                    i,
                    t.to_f64().unwrap(),
                    d_prev.to_f64().unwrap(),
                    d_cur.to_f64().unwrap()
                );
            }

            t = t + delta;
        }

        // Safety hatch - looks like a solution won't converge with these inputs
        panic!("{}", MAX_ITERATIONS_MSG);
    }

    /// Gradient descent fallback for calculating vertical lines differences.
    ///
    /// This method should be reliable but slow and can be used for testing purposes as a reference implementation for
    /// prop testing or to find roots manually for unit tests.
    /// TODO: Make sure this works well and converges all the time
    pub(super) fn gradient_descent_vline_vline<T: GeoFloat>(
        l1: &Line<T>,
        l2: &Line<T>,
    ) -> (T, Point<T>, Point<T>) {
        let tolerance = tolerance::<T>() * T::from(0.001).unwrap();
        let h = T::from(1e-8).unwrap();
        let two = T::one() + T::one();
        let mut learning_rate = T::from(0.01).unwrap();

        let mut t1 = T::from(0.5).unwrap();
        let mut t2 = T::from(0.5).unwrap();
        let mut pt1 = l1.start_point();
        let mut pt2 = l2.start_point();
        let mut d = T::from(9.0).unwrap(); // As long as it's bigger than PI it should be fine

        if DEBUG_DISPLAY_TEST {
            eprintln!("   i: t1       t2       | pt1_lat   pt2_lat   | d_cur    lr");
        }

        for i in 0..=10000 {
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
                learning_rate = learning_rate * T::from(1.1).unwrap();
            } else {
                learning_rate = learning_rate * T::from(0.5).unwrap();
            }

            t1 = t1_new;
            t2 = t2_new;
            d = d_new;

            if DEBUG_DISPLAY_TEST {
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

    mod point_vertical_line {
        use super::*;

        #[test]
        fn point_directly_west() {
            // Line: longitude 0°, latitude 0° to 30°
            // Point: longitude -45°, latitude 15°
            let lat_min = 0.0;
            let lat_max = PI / 6.0; // 30°
            let lon = 0.0; // 0°
            let point = Point::new(-PI / 12.0, PI / 12.0); // -45°, 15°
            let (d, _) = vertical_line_to_point(lat_min, lat_max, lon, &point);

            // Expected: distance to point from numerical solution
            let expected_point = Point::new(0.0, 0.2706);
            let expected = haversine(&point, &expected_point);

            assert_abs_diff_eq!(d, expected, epsilon = 1e-6);
        }

        #[test]
        fn point_directly_east() {
            // Line: longitude 90°, latitude -20° to 20°
            // Point: longitude 120°, latitude 0°
            let lat_min = -PI / 9.0; // -20°
            let lat_max = PI / 9.0; // 20°
            let lon = PI / 2.0; // 90°
            let point = Point::new(2.0 * PI / 3.0, 0.0); // 120°, 0°
            let (d, _) = vertical_line_to_point(lat_min, lat_max, lon, &point);

            // Expected: distance to point (90°, 0°)
            let expected_point = Point::new(PI / 2.0, 0.0);
            let expected = haversine(&point, &expected_point);

            assert_abs_diff_eq!(d, expected, epsilon = 1e-6);
        }

        #[test]
        fn closest_point_at_line_midpoint() {
            // Line: longitude 0°, latitude -10° to 10°
            // Point: longitude 45°, latitude 0° (equidistant from endpoints)
            let lat_min = -PI / 18.0; // -10°
            let lat_max = PI / 18.0; // 10°
            let lon = 0.0; // 0°
            let point = Point::new(PI / 4.0, 0.0); // 45°, 0°
            let (d, _) = vertical_line_to_point(lat_min, lat_max, lon, &point);

            // Expected: distance to midpoint (0°, 0°)
            let expected_point = Point::new(0.0, 0.0);
            let expected = haversine(&point, &expected_point);

            assert_abs_diff_eq!(d, expected, epsilon = 1e-6);
        }

        #[test]
        fn antimeridian_crossing() {
            // Line: longitude 179°, latitude -30° to 30°
            // Point: longitude -170° (10° west of antimeridian), latitude 0°
            let lat_min = -PI / 6.0; // -30°
            let lat_max = PI / 6.0; // 30°
            let lon = 179.0 * PI / 180.0; // 179°
            let point = Point::new(-170.0 * PI / 180.0, 0.0); // -170°, 0°
            let (d, _) = vertical_line_to_point(lat_min, lat_max, lon, &point);

            // Expected: distance to point (179°, 0°)
            // TODO: Convert this to PI - DEGREE (and do same above
            let expected_point = Point::new(179.0 * PI / 180.0, 0.0);
            let expected = haversine(&point, &expected_point);

            assert_abs_diff_eq!(d, expected, epsilon = 1e-6);
        }

        #[test]
        fn high_latitude_line() {
            // Line: longitude 0°, latitude 60° to 80°
            // Point: longitude 90°, latitude 70°
            let lat_min = PI / 3.0; // 60°
            let lat_max = 4.0 * PI / 9.0; // 80°
            let lon = 0.0; // 0°
            let point = Point::new(PI / 2.0, 7.0 * PI / 18.0); // 90°, 70°
            let (d, _) = vertical_line_to_point(lat_min, lat_max, lon, &point);

            // Expected: distance to point (0°, 80°)
            let expected_point = Point::new(0.0, 4.0 * PI / 9.0);
            let expected = haversine(&point, &expected_point);

            assert_abs_diff_eq!(d, expected, epsilon = 1e-6);
        }

        #[test]
        fn southern_hemisphere() {
            // Line: longitude -90°, latitude -60° to -30°
            // Point: longitude -120°, latitude -45°
            let lat_min = -PI / 3.0; // -60°
            let lat_max = -PI / 6.0; // -30°
            let lon = -PI / 2.0; // -90°
            let point = Point::new(-2.0 * PI / 3.0, -PI / 4.0); // -120°, -45°
            let (d, _) = vertical_line_to_point(lat_min, lat_max, lon, &point);

            // Expected: distance to point (-90°, -45°)
            let expected_point = Point::new(-PI / 2.0, -0.8571);
            let expected = haversine(&point, &expected_point);

            assert_abs_diff_eq!(d, expected, epsilon = 1e-6);
        }

        #[test]
        fn close_longitude_long_segment() {
            let lat_min = 0.0;
            let lat_max = 1.0;
            let lon = 0.1;
            let point = Point::new(0.11, 0.2);
            let (d, _) = vertical_line_to_point(lat_min, lat_max, lon, &point);

            let (expected, _) = gradient_descent_vline_pt(lat_min, lat_max, lon, &point);

            assert_abs_diff_eq!(d, expected, epsilon = 1e-6);
        }
    }

    mod vertical_line_line {
        use super::*;

        #[test]
        fn parallel_vertical_lines_same_longitude() {
            // Two vertical lines at the same longitude should have zero distance
            let l1 = Line::new((0.0, -0.3), (0.0, 0.3));
            let l2 = Line::new((0.0, -0.5), (0.0, 0.5));

            let d = vertical_line_to_line(&l1, &l2);
            assert_abs_diff_eq!(d, 0.0, epsilon = 1e-6);
        }

        #[test]
        fn parallel_vertical_lines_different_longitude() {
            // Two vertical lines at different longitudes (radians)
            let l1 = Line::new((0.0, -1.0), (0.0, 1.0));
            let l2 = Line::new((DEGREE, -1.0), (DEGREE, 1.0));

            let distance = vertical_line_to_line(&l1, &l2);
            eprintln!("{}", distance);

            // Expected distance should be roughly the great circle distance at the equator
            let expected = haversine(&Point::new(0.0, 0.0), &Point::new(DEGREE, 0.0));
            assert_abs_diff_eq!(distance, expected, epsilon = 1e-6);
        }

        #[test]
        fn non_overlapping_vertical_lines() {
            // Lines at different latitudes that don't overlap (radians)
            let l1 = Line::new((0.0, -0.2), (0.0, -0.1)); // Southern line
            let l2 = Line::new((0.01745, 0.1), (0.01745, 0.2)); // Northern line, ~1 degree east

            let distance = vertical_line_to_line(&l1, &l2);

            // Should be distance between closest endpoints
            let expected = haversine(&Point::new(0.0, -0.1), &Point::new(0.01745, 0.1));
            assert_abs_diff_eq!(distance, expected, epsilon = 1e-6);
        }

        #[test]
        fn line_line_antimeridian_crossing() {
            let half = DEGREE / 2.0;
            // One line near π, another near -π (crossing antimeridian)
            let l1 = Line::new((PI - half, -0.1), (PI - half, 0.1)); // ~179.5° in radians
            let l2 = Line::new((-PI + half, -0.1), (-PI + half, 0.1)); // ~-179.5° in radians

            let distance = vertical_line_to_line(&l1, &l2);

            // The shortest distance should be across the antimeridian (~1°)
            // not the long way around (~359°)
            let expected = haversine(&Point::new(PI - half, 0.0), &Point::new(-PI + half, 0.0));
            assert_abs_diff_eq!(distance, expected, epsilon = 1e-6);

            // Sanity check: should be much less than halfway around the world
            assert!(distance < PI / 2.0);
        }

        #[test]
        fn offset_vertical_lines() {
            // Lines that are offset in both longitude and latitude (radians)
            let l1 = Line::new((0.0, 0.0), (0.0, 0.05));
            let l2 = Line::new((DEGREE, 0.02), (DEGREE, 0.1));

            let distance = vertical_line_to_line(&l1, &l2);

            // The closest points should be somewhere in the overlapping latitude range
            // Which we add as an assert for a sanity check, but use grad desc as primary solution
            let (expected, p1, p2) = gradient_descent_vline_vline(&l1, &l2);
            assert_abs_diff_eq!(distance, expected, epsilon = 1e-6);

            assert!(p1.y() >= 0.0 && p1.y() <= 0.05);
            assert!(p2.y() >= 0.02 && p2.y() <= 0.1);
        }

        // FIX: Make this test work
        #[test]
        fn offset_vertical_lines_northern_hemisphere() {
            // Lines offset in both longitude and latitude, away from equator and further apart
            let l1 = Line::new((0.0, 35.0 * DEGREE), (0.0, 70.0 * DEGREE)); // 0° lon, 35° to 70° lat
            let l2 = Line::new((5.0 * DEGREE, 50.0 * DEGREE), (5.0 * DEGREE, 80.0 * DEGREE)); // ~5° lon, 50° to 80° lat

            let distance = vertical_line_to_line(&l1, &l2);

            // The closest points should be in the overlapping latitude range (50° to 70°)
            let (expected, _, _) = gradient_descent_vline_vline(&l1, &l2);
            assert_abs_diff_eq!(distance, expected, epsilon = 1e-6);
        }
    }
}
