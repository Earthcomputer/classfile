import java.lang.annotation.ElementType;

public class TestSyntheticClass {
    void test(ElementType element) {
        switch (element) {
            case FIELD: System.out.println("field"); break;
            case METHOD: System.out.println("method"); break;
            default: System.out.println("other"); break;
        }
    }
}
