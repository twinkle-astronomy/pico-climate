use picoserve::response::chunked::ChunkWriter;

use crate::prometheus::Sample;


pub(super) struct MetricSamples<'a, const LABELS: usize, I> where I: Iterator<Item = &'a Sample<LABELS>> {
    labels: [&'static str; LABELS],
    samples: I,
}

impl<'a, const LABELS: usize, I> MetricSamples<'a, LABELS, I> where I: Iterator<Item = &'a Sample<LABELS>> {
    pub(super) const fn new(labels: [&'static str; LABELS], samples: I) -> Self {
        MetricSamples { labels, samples }
    }
}

impl<'a, const LABELS: usize, I> MetricSamples<'a, LABELS, I> where I: Iterator<Item = &'a Sample<LABELS>> {
    pub(super) async fn write_chunks<W: picoserve::io::Write>(
        self,
        name: &'static str,
        chunk_writer: &mut ChunkWriter<W>,
    ) -> Result<(), W::Error> {
        for sample in self.samples {
            write!(chunk_writer, "{}{}", name, "{").await?;
            for (i, (label_name, label_value)) in self
                .labels
                .iter()
                .zip(sample.label_values.iter())
                .enumerate()
            {
                if i > 0 {
                    write!(chunk_writer, ",").await?;
                }
                write!(chunk_writer, "{}=\"{}\"", label_name, label_value).await?;
                chunk_writer.flush().await?;
            }
            chunk_writer.flush().await?;
            write!(chunk_writer, "{}", "}").await?;
            chunk_writer.flush().await?;
            write!(
                chunk_writer,
                " {}\n",
                sample.value.load(core::sync::atomic::Ordering::SeqCst)
            )
            .await?;
            chunk_writer.flush().await?;
        }
        Ok(())
    }
}
