module a::m {
    public struct X<phantom T: copy>() has copy, drop;

    fun mycopy<T: copy>(t: &T): T {
        *t
    }

    macro fun needs_copy<$T, $U, $V>(_: X<$T>, $u: $U, $v: $V): X<$U> {
        $u;
        mycopy(&$v);
        X()
    }

    fun t() {
        needs_copy!(X<u64>(), 0u64, @0);
    }
}