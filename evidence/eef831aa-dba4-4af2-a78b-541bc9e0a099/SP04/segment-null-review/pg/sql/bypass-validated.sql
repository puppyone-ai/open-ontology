SELECT convalidated::text || '|' || pg_get_constraintdef(oid) FROM pg_constraint WHERE conname = 'apply_receipts_segment_check';
