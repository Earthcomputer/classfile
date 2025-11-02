import java.lang.annotation.ElementType;
import java.lang.annotation.Retention;
import java.lang.annotation.RetentionPolicy;

@Retention(RetentionPolicy.CLASS)
public @interface InvisibleAnnotation {
    boolean[] booleans() default {};
    byte[] bytes() default {};
    char[] chars() default {};
    short[] shorts() default {};
    int[] ints() default {};
    long[] longs() default {};
    float[] floats() default {};
    double[] doubles() default {};
    String[] strings() default {};
    Class<?>[] classes() default {};
    ElementType[] enums() default {};
    Deprecated[] annotations() default {};
}
