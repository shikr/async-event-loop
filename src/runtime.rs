use priority_queue::PriorityQueue;
use rquickjs::context::EvalOptions;
use rquickjs::{AsyncContext, AsyncRuntime, Ctx, Error, Function, Persistent, Result};
use std::{cell::RefCell, cmp::Reverse, path::Path, rc::Rc};
use tokio::runtime::Runtime as TokioRuntime;
use tokio::time::{Duration, Instant};

type CallStack = Rc<RefCell<PriorityQueue<Persistent<Function<'static>>, Reverse<Instant>>>>;

pub struct Runtime {
    runtime: TokioRuntime,
    qruntime: AsyncRuntime,
    context: AsyncContext,
    pq: CallStack,
}

impl Runtime {
    pub fn new() -> Result<Self> {
        let runtime = TokioRuntime::new()?;
        let pq = Rc::new(RefCell::new(PriorityQueue::new()));

        let (qruntime, context) = runtime.block_on(async {
            let qruntime = AsyncRuntime::new()?;
            let context = AsyncContext::full(&qruntime).await?;

            context
                .with(|ctx| {
                    let globals = ctx.globals();
                    globals.set(
                        "setTimeout",
                        Function::new(ctx.clone(), Self::register_callback(pq.clone())),
                    )?;

                    globals.set(
                        "print",
                        Function::new(ctx, |value: String| {
                            println!("{}", value);
                        }),
                    )?;

                    Result::Ok(())
                })
                .await?;

            Result::Ok((qruntime, context))
        })?;

        Ok(Self {
            runtime,
            qruntime,
            context,
            pq,
        })
    }

    pub fn new_with_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let this = Self::new()?;
        let context = &this.context;

        let mut options = EvalOptions::default();
        options.promise = true;

        this.runtime.block_on(async {
            context
                .with(|ctx| ctx.eval_file_with_options::<(), P>(path, options))
                .await
        })?;

        Ok(this)
    }

    pub fn run(&self) -> Result<()> {
        loop {
            match self.runtime.block_on(async {
                match self.qruntime.execute_pending_job().await {
                    Err(_) => return Err(Error::Exception),
                    Ok(v) => {
                        if v {
                            return Ok(false);
                        }
                    }
                };

                let binding = self.pq.clone();
                let mut pq = binding.borrow_mut();
                if let Some((callback, Reverse(timeout))) = pq.pop() {
                    tokio::time::sleep_until(timeout).await;
                    self.context
                        .with(|ctx| {
                            let func = callback.restore(&ctx)?;
                            func.call::<_, ()>(())?;

                            Result::Ok(())
                        })
                        .await?;

                    return Ok(false);
                }

                Ok(true)
            }) {
                Ok(true) => return Ok(()),
                Ok(_) => {}
                Err(e) => return Err(e),
            };
        }
    }

    fn register_callback(pq: CallStack) -> impl for<'js> Fn(Ctx<'js>, Function<'js>, u32) {
        move |ctx, func, timeout| {
            let persistent = Persistent::save(&ctx, func);
            pq.borrow_mut().push(
                persistent,
                Reverse(Instant::now() + Duration::from_millis(timeout as u64)),
            );
        }
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        self.pq.borrow_mut().clear();
    }
}
