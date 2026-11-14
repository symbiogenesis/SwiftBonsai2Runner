use super::*;
use crate::{CacheOwner, DispatchIdentity};

fn fixture() -> Result<(HybridPlan, Q8Matmul, Q8Buffer, Arc<AtomicBool>), Error> {
    let projection = Q8Matmul::new(128, 32, 2, 100_000)?;
    let identity = DispatchIdentity {
        model: [1; 32],
        model_bytes: 100_000,
        module: [2; 32],
        catalog: [3; 32],
        driver: [4; 32],
        source: [5; 20],
    };
    let plan = HybridPlan::new(
        identity,
        &[
            TensorPlacement::Cpu {
                tensor: 1,
                offset: 0,
                bytes: 32,
            },
            TensorPlacement::Htp0Q8 {
                tensor: 2,
                offset: 32,
                projection,
            },
        ],
        CacheOwner::Cpu,
    )?;
    let buffer = Q8Buffer::new(
        7,
        u32::try_from(projection.allocation_bytes()).map_err(|_| Error::Contract)?,
        0,
        0x90,
        4096,
    )?;
    Ok((plan, projection, buffer, Arc::new(AtomicBool::new(false))))
}

fn reply(sequence: u64) -> [u8; 88] {
    // Independent literal response fields from the pinned public wire layout.
    let mut bytes = [0; 88];
    bytes[..8].copy_from_slice(&sequence.to_le_bytes());
    for (offset, value) in [(24, 1_u32), (28, 1), (32, 3), (36, 1)] {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    bytes
}

fn result_image(projection: Q8Matmul, image: &mut [u8]) {
    // A transport simulation, not Q8 DSP execution or an arithmetic oracle.
    for value in image[projection.output_offset()..][..projection.rows() * projection.batch() * 4]
        .as_chunks_mut::<4>()
        .0
    {
        *value = 7.0_f32.to_le_bytes();
    }
}

fn completion<'a>(buffer: Q8Buffer, response: &'a [u8], image: &'a [u8]) -> Q8Completion<'a> {
    Q8Completion {
        read_result: 0,
        flags: 3,
        buffer_count: 1,
        buffer,
        response,
        image,
    }
}

#[test]
fn q8_dispatch_selected_immutable_tensor_correlated_output_and_fixed_budget() -> Result<(), Error> {
    let (plan, projection, buffer, cancellation) = fixture()?;
    let mut dispatch = Q8Dispatcher::new(
        &plan,
        2,
        buffer,
        2,
        Instant::now() + Duration::from_secs(5),
        cancellation,
    )?;
    assert!(std::ptr::eq(
        std::ptr::from_ref(dispatch.plan()),
        std::ptr::from_ref(&plan)
    ));
    let mut image = vec![0; projection.allocation_bytes()];
    let weights = vec![0; projection.weight_bytes()];
    let mut output = [99.0; 64];
    for sequence in 1_u64..=2 {
        let request = dispatch.prepare(&mut image, &weights, &[0.0; 256])?;
        assert_eq!(request.wire()[..8], sequence.to_le_bytes());
        assert_eq!(request.buffer(), buffer);
        dispatch.record_write(0)?;
        result_image(projection, &mut image);
        dispatch.complete(
            completion(buffer, &reply(sequence), &image),
            &mut output,
            |values| {
                if values == [7.0; 64] {
                    Ok(())
                } else {
                    Err(Error::Contract)
                }
            },
        )?;
        assert_eq!(output, [7.0; 64]);
    }
    assert_eq!((dispatch.attempted(), dispatch.completed()), (2, 2));
    assert!(dispatch.prepare(&mut image, &weights, &[0.0; 256]).is_err());
    assert!(dispatch.is_terminal());
    Ok(())
}

#[test]
fn q8_dispatch_refused_write_cannot_be_erased_by_valid_numeric_reply() -> Result<(), Error> {
    let (plan, projection, buffer, cancellation) = fixture()?;
    let mut dispatch = Q8Dispatcher::new(
        &plan,
        2,
        buffer,
        2,
        Instant::now() + Duration::from_secs(5),
        cancellation,
    )?;
    let mut image = vec![0; projection.allocation_bytes()];
    let weights = vec![0; projection.weight_bytes()];
    dispatch.prepare(&mut image, &weights, &[0.0; 256])?;
    assert!(matches!(
        dispatch.record_write(14),
        Err(Error::Native("q8-queue-write", 14))
    ));
    result_image(projection, &mut image);
    let mut output = [99.0; 64];
    assert!(
        dispatch
            .complete(completion(buffer, &reply(1), &image), &mut output, |_| Ok(
                ()
            ))
            .is_err()
    );
    assert_eq!(output, [99.0; 64]);
    assert!(dispatch.prepare(&mut image, &weights, &[0.0; 256]).is_err());
    assert_eq!((dispatch.attempted(), dispatch.completed()), (1, 0));
    Ok(())
}

#[test]
fn q8_dispatch_stale_corrupt_extent_cache_and_numeric_replies_are_terminal() -> Result<(), Error> {
    let (plan, projection, buffer, cancellation) = fixture()?;
    let weights = vec![0; projection.weight_bytes()];
    for case in 0..12 {
        let mut dispatch = Q8Dispatcher::new(
            &plan,
            2,
            buffer,
            2,
            Instant::now() + Duration::from_secs(5),
            Arc::clone(&cancellation),
        )?;
        let mut image = vec![0; projection.allocation_bytes()];
        dispatch.prepare(&mut image, &weights, &[0.0; 256])?;
        dispatch.record_write(0)?;
        result_image(projection, &mut image);
        if case == 10 {
            image[projection.output_offset() + 63 * 4..][..4]
                .copy_from_slice(&f32::NAN.to_le_bytes());
        }
        let mut response = reply(u64::from(case != 0));
        if case == 1 {
            response[36] = 0;
        }
        let mut observed = completion(buffer, &response, &image);
        match case {
            2 => observed.read_result = 516,
            3 => observed.flags = 2,
            4 => observed.flags = 0x23,
            5 => observed.buffer_count = 2,
            6 => observed.buffer = Q8Buffer::new(8, buffer.bytes(), 0, 0x90, 4096)?,
            7 => observed.buffer = Q8Buffer::new(7, buffer.bytes(), 0, 0x90, 8192)?,
            8 => observed.response = &response[..87],
            9 => observed.image = &image[..image.len() - 1],
            _ => {}
        }
        let mut output = [99.0; 64];
        assert!(
            dispatch
                .complete(observed, &mut output, |_| if case == 11 {
                    Err(Error::Contract)
                } else {
                    Ok(())
                })
                .is_err()
        );
        assert_eq!(output, [99.0; 64]);
        assert!(dispatch.is_terminal());
        assert!(dispatch.prepare(&mut image, &weights, &[0.0; 256]).is_err());
        assert_eq!((dispatch.attempted(), dispatch.completed()), (1, 0));
    }
    Ok(())
}

#[test]
fn q8_dispatch_cancellation_before_write_and_during_oracle_publishes_nothing() -> Result<(), Error>
{
    let (plan, projection, buffer, cancellation) = fixture()?;
    let weights = vec![0; projection.weight_bytes()];
    for during_oracle in [false, true] {
        cancellation.store(false, Ordering::Release);
        let mut dispatch = Q8Dispatcher::new(
            &plan,
            2,
            buffer,
            2,
            Instant::now() + Duration::from_secs(5),
            Arc::clone(&cancellation),
        )?;
        let mut image = vec![0xaa; projection.allocation_bytes()];
        let mut output = [99.0; 64];
        if during_oracle {
            dispatch.prepare(&mut image, &weights, &[0.0; 256])?;
            dispatch.record_write(0)?;
            result_image(projection, &mut image);
            assert!(
                dispatch
                    .complete(completion(buffer, &reply(1), &image), &mut output, |_| {
                        cancellation.store(true, Ordering::Release);
                        Ok(())
                    })
                    .is_err()
            );
        } else {
            cancellation.store(true, Ordering::Release);
            assert!(dispatch.prepare(&mut image, &weights, &[0.0; 256]).is_err());
            assert!(image.iter().all(|&value| value == 0xaa));
        }
        assert_eq!(output, [99.0; 64]);
        assert!(dispatch.is_terminal());
        assert_eq!(dispatch.completed(), 0);
    }
    Ok(())
}

#[test]
fn q8_dispatch_oracle_unwind_cannot_reopen_mapping_or_publish_output() -> Result<(), Error> {
    let (plan, projection, buffer, cancellation) = fixture()?;
    let mut dispatch = Q8Dispatcher::new(
        &plan,
        2,
        buffer,
        2,
        Instant::now() + Duration::from_secs(5),
        cancellation,
    )?;
    let weights = vec![0; projection.weight_bytes()];
    let mut image = vec![0; projection.allocation_bytes()];
    dispatch.prepare(&mut image, &weights, &[0.0; 256])?;
    dispatch.record_write(0)?;
    result_image(projection, &mut image);
    let mut output = [99.0; 64];
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        dispatch.complete(completion(buffer, &reply(1), &image), &mut output, |_| {
            std::panic::resume_unwind(Box::new("independent-oracle"));
        })
    }));
    assert!(outcome.is_err());
    assert!(dispatch.is_terminal());
    assert_eq!(output, [99.0; 64]);
    assert!(dispatch.prepare(&mut image, &weights, &[0.0; 256]).is_err());
    Ok(())
}

#[test]
fn q8_dispatch_rejects_unselected_cpu_wrong_mapping_and_original_expiry() -> Result<(), Error> {
    let (plan, projection, buffer, cancellation) = fixture()?;
    for (tensor, operations, deadline) in [
        (1, 1, Instant::now() + Duration::from_secs(5)),
        (3, 1, Instant::now() + Duration::from_secs(5)),
        (2, 0, Instant::now() + Duration::from_secs(5)),
        (2, 24_001, Instant::now() + Duration::from_secs(5)),
        (2, 1, Instant::now()),
        (2, 1, Instant::now() + Duration::from_secs(121)),
    ] {
        assert!(
            Q8Dispatcher::new(
                &plan,
                tensor,
                buffer,
                operations,
                deadline,
                Arc::clone(&cancellation)
            )
            .is_err()
        );
    }
    let wrong = Q8Buffer::new(7, buffer.bytes() + 4096, 0, 0x90, 4096)?;
    assert!(
        Q8Dispatcher::new(
            &plan,
            2,
            wrong,
            1,
            Instant::now() + Duration::from_secs(5),
            Arc::clone(&cancellation)
        )
        .is_err()
    );
    let mut dispatch = Q8Dispatcher::new(
        &plan,
        2,
        buffer,
        1,
        Instant::now() + Duration::from_secs(5),
        cancellation,
    )?;
    let weights = vec![0; projection.weight_bytes()];
    let mut image = vec![0xaa; projection.allocation_bytes()];
    dispatch.work_deadline = Instant::now();
    assert!(dispatch.prepare(&mut image, &weights, &[0.0; 256]).is_err());
    assert!(image.iter().all(|&value| value == 0xaa));
    assert_eq!(dispatch.attempted(), 0);
    Ok(())
}
