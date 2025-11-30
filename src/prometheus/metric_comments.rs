use picoserve::response::chunked::ChunkWriter;

use crate::prometheus::MetricType;

pub(super) struct MetricComments<'a> {
    help: &'a str,
    metric_type: MetricType,
}

impl<'a> MetricComments<'a> {
    pub(super) const fn new(help: &'a str, metric_type: MetricType) -> Self {
        Self { help, metric_type }
    }

    pub(super) async fn write_chunks<W: picoserve::io::Write>(
        &self,
        name: &'a str,
        chunk_writer: &mut ChunkWriter<W>,
    ) -> Result<(), W::Error> {
        write!(chunk_writer, "# HELP {} {}\n", name, self.help).await?;
        chunk_writer.flush().await?;
        write!(
            chunk_writer,
            "# TYPE {} {}\n",
            name,
            self.metric_type.to_str()
        )
        .await?;
        Ok(())
    }
}
