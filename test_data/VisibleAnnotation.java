import java.lang.annotation.ElementType;
import java.lang.annotation.Retention;
import java.lang.annotation.RetentionPolicy;

@Retention(RetentionPolicy.RUNTIME)
public @interface VisibleAnnotation {
    boolean booleanValue() default false;
    byte byteValue() default 0;
    char charValue() default '\0';
    short shortValue() default 0;
    int intValue() default 0;
    long longValue() default 0;
    float floatValue() default 0;
    double doubleValue() default 0;
    String stringValue() default "";
    Class<?> classValue() default void.class;
    ElementType enumValue() default ElementType.TYPE;
    Deprecated annotationValue() default @Deprecated;
}
