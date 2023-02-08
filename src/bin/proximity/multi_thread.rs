use geo_munge::qt::Quadtree;
use rayon::prelude::*;

use crate::{
    run::{run_find, run_output},
    CsvReader, CsvWriter, InputSettings,
};

pub(super) fn exec_multi_thread(
    csv_reader: CsvReader,
    mut csv_writer: CsvWriter,
    qt: &Quadtree,
    settings: &InputSettings,
) {
    // We set up a thread scope to run the work in parallel with the output. We need a scipe rather
    // than just spawning a thread for one of the two tasks so that we don't run into issues with
    // thread lifetimes and moving values. We don't use Rayon's join either because the channel may
    // cause join to deadlock. The scope also ensures all the threads are closed before returning.
    //
    // TODO: The metadata processing is currently done in printing, which is using a single
    // thread. Should this be rearchitected to deal with meta inside the parallel iterator? This
    // may only may only make sense if the meta stuff takes a while (test with flamegraph)
    rayon::scope(|s| {
        // Use channel to send the find output to the printing routine
        let (sender, receiver) = std::sync::mpsc::channel();

        s.spawn(|_| {
            // Use a bridge to parallelize after reading and a channel to re-serialize after the
            // computation
            csv_reader
                .into_records()
                .enumerate()
                .par_bridge()
                .for_each_with(sender, |s, enum_record| {
                    let output = run_find(enum_record, &qt, &settings);
                    s.send(output)
                        .expect("Receiver closed unexpectedly, aborting");
                });
        });

        s.spawn(|_| {
            receiver
                .into_iter()
                .for_each(|output| run_output(&mut csv_writer, settings, output));
        });
    });
}
