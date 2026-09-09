// Copyright 2026 STARGA Inc.
// Licensed under the Apache License, Version 2.0.

macro_rules! mind_cf {
    ($($body:tt)*) => {
        #[cfg(feature = "cross-module-imports")]
        {
            $($body)*;
        }
    };
}
