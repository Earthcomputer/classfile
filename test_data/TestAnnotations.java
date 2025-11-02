import java.lang.annotation.ElementType;
import java.lang.annotation.Retention;
import java.lang.annotation.RetentionPolicy;
import java.lang.annotation.Target;
import java.util.function.Supplier;

@VisibleAnnotation(
    booleanValue = true,
    byteValue = 1,
    charValue = 'a',
    shortValue = 2,
    intValue = 3,
    longValue = 4,
    floatValue = 5,
    doubleValue = 6,
    stringValue = "Hello World",
    classValue = String.class,
    enumValue = ElementType.FIELD,
    annotationValue = @Deprecated(forRemoval = true)
)
@InvisibleAnnotation(
    booleans = {false, true},
    bytes = {0},
    chars = {'a', 'b'},
    shorts = {1},
    ints = {1, 2},
    longs = {42, 69},
    floats = {420.69f},
    doubles = {-100},
    strings = {"Hello", "World"},
    classes = {Class.class, void.class, String[].class},
    enums = {ElementType.FIELD, ElementType.METHOD},
    annotations = {@Deprecated, @Deprecated}
)
public abstract class TestAnnotations<
    @VisibleTypeAnnotation T extends @InvisibleTypeAnnotation Object,
    U extends Supplier<@VisibleTypeAnnotation Object>,
    V extends Supplier<@InvisibleTypeAnnotation ?>,
    W extends Supplier<? extends @VisibleTypeAnnotation Object>,
    X extends Supplier<? super @VisibleTypeAnnotation Object>
> extends @VisibleTypeAnnotation Object implements @InvisibleTypeAnnotation Runnable {

}
