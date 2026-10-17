def invalidate_cache_entry(cache, key):
    """Remove one stale cached value for the supplied key."""
    cache.pop(key, None)
