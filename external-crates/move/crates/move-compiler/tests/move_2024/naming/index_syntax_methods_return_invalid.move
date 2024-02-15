#[defines_primitive(vector)]
module std::vector {
    #[bytecode_instruction]
    native public fun empty<Element>(): vector<Element>;

    #[bytecode_instruction]
    native public fun borrow<Element>(v: &vector<Element>, i: u64): &Element;

    #[bytecode_instruction]
    native public fun borrow_mut<Element>(v: &mut vector<Element>, i: u64): &mut Element;
}

// Incorrect usage

module a::invalid0 {

    use std::vector;

    public struct S has drop { t: vector<u64> }

    #[allow(unused_variable)]
    #[syntax(index)]
    public fun borrow_t(s: &S, i: u64): &mut u64 { abort 0 }

    public struct S2 has drop { t: vector<u64> }

    #[syntax(index)]
    public fun borrow_t2(s: &S2, i: u64): u64 {
        *vector::borrow(&s.t, i)
    }

    public struct S3 has drop { t: vector<u64> }

    #[syntax(index)]
    public fun borrow_t3(s: &S3, i: u64): bool {
        vector::borrow(&s.t, i) == &5
    }

}

module a::invalid1 {

    use std::vector;

    public struct S has drop { t: vector<u64> }

    #[syntax(index)]
    public fun borrow_t(s: &mut S, i: u64): &u64 {
        vector::borrow_mut(&mut s.t, i)
    }

    public struct S2 has drop { t: vector<u64> }

    #[syntax(index)]
    public fun borrow_t2(s: &mut S2, i: u64): u64 {
        *vector::borrow_mut(&mut s.t, i)
    }

    public struct S3 has drop { t: vector<u64> }

    #[syntax(index)]
    public fun borrow_t3(s: &mut S3, i: u64): bool {
        vector::borrow_mut(&mut s.t, i) == &mut 5
    }

}

module a::invalid2 {

    public struct S<T> has drop { t: vector<T> }

    #[allow(unused_variable)]
    #[syntax(index)]
    public fun borrow_t<T>(s: &S<T>, i: u64): &mut u64 { abort 0 }

    public struct S2<T> has drop { t: vector<T> }

    #[allow(unused_variable)]
    #[syntax(index)]
    public fun borrow_t2<T>(s: &S2<T>, i: u64): T { abort 0 }

    public struct S3<T> has drop { t: vector<T> }

    #[allow(unused_variable)]
    #[syntax(index)]
    public fun borrow_t3<T>(s: &S3<T>, i: u64): bool { abort 0 }

}

module a::invalid3 {

    public struct S<T> has drop { t: vector<T> }

    #[allow(unused_variable)]
    #[syntax(index)]
    public fun borrow_t<T>(s: &mut S<T>, i: u64): &u64 { abort 0 }

    public struct S2<T> has drop { t: vector<T> }

    #[allow(unused_variable)]
    #[syntax(index)]
    public fun borrow_t2<T>(s: &mut S2<T>, i: u64): T { abort 0 }

    public struct S3<T> has drop { t: vector<T> }

    #[allow(unused_variable)]
    public fun borrow_t3<T>(s: &mut S3<T>, i: u64): bool { abort 0 }

}
