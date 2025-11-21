use http_body::Frame;

/// A wrapper around [`http_body::Frame`] that implements [`Clone`].
///
/// HTTP frames are not cloneable by default, but `SharedBody` needs to clone frames
/// to distribute them to multiple consumers. This wrapper enables cloning by
/// reconstructing the frame from its data or trailers.
///
/// # Implementation Note
///
/// The clone implementation checks whether the frame contains data or trailers
/// and reconstructs the appropriate frame type. This is necessary because
/// `Frame<D>` itself doesn't implement `Clone` even when `D: Clone`.
pub(crate) struct ClonableFrame<D> {
    frame: Frame<D>,
}

impl<D: Clone> Clone for ClonableFrame<D>
where
    D: Clone,
{
    fn clone(&self) -> Self {
        ClonableFrame {
            frame: match self.frame.data_ref() {
                Some(data) => Frame::data(data.clone()),
                None => Frame::trailers(self.frame.trailers_ref().unwrap().clone()),
            },
        }
    }
}

impl<D> ClonableFrame<D>
where
    D: Clone,
{
    /// Wraps a frame to make it cloneable.
    pub(crate) fn new(frame: Frame<D>) -> Self {
        ClonableFrame { frame }
    }
}

impl<D> From<ClonableFrame<D>> for Frame<D>
where
    D: Clone,
{
    /// Unwraps the cloneable frame back into the original frame.
    fn from(clonable: ClonableFrame<D>) -> Self {
        clonable.frame
    }
}
