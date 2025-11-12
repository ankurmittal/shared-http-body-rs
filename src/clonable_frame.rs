use http_body::Frame;

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
    pub(crate) fn new(frame: Frame<D>) -> Self {
        ClonableFrame { frame }
    }
}

impl<D> From<ClonableFrame<D>> for Frame<D>
where
    D: Clone,
{
    fn from(clonable: ClonableFrame<D>) -> Self {
        clonable.frame
    }
}
