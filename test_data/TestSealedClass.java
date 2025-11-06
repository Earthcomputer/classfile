
public abstract sealed class TestSealedClass permits TestSealedClass.Foo, TestSealedClass.Bar {
    public static final class Foo extends TestSealedClass {}
    public static final class Bar extends TestSealedClass {}
}
