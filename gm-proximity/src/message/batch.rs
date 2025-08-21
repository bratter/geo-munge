use anyhow::Result;

/// Function to manage batching over an iterator of items that will be compiled and send in the process_batch call.
///
/// This function works for fixed size batches and will return the number of batches processed, including the final
/// flush it is occurs.
pub fn dispatch_counted_batches<T, R, I, F>(
    iter: I,
    batch_size: usize,
    mut process_batch: F,
) -> Result<usize>
where
    I: Iterator<Item = T>,
    F: FnMut(Vec<R>) -> Result<()>,
    R: From<T>,
{
    let mut buffer = Vec::with_capacity(batch_size);
    let mut batch_count = 0;

    for item in iter {
        if buffer.len() >= batch_size {
            let batch = std::mem::replace(&mut buffer, Vec::with_capacity(batch_size));
            process_batch(batch)?;
            batch_count += 1;
        }

        buffer.push(R::from(item));
    }

    // Final flush
    if !buffer.is_empty() {
        process_batch(buffer)?;
        batch_count += 1;
    }

    Ok(batch_count)
}
