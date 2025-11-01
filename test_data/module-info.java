module test {
    requires java.base;
    requires static java.logging;
    requires transitive java.net.http;
    exports pkg;
    opens pkg2;
    exports pkg2 to java.base;
    opens pkg to java.base;
    uses Runnable;
    provides Runnable with pkg.ClassInPackage;
}