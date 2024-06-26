FROM ghcr.io/graalvm/native-image-community:21-ol9 as build
RUN microdnf install findutils # Gradle 8.7 requires xargs
COPY . /home/gradle/src
WORKDIR /home/gradle/src
RUN ./gradlew micronaut-jdbc:nativeCompile -x test --no-daemon

FROM cgr.dev/chainguard/wolfi-base:latest
WORKDIR /micronaut
COPY --from=build /home/gradle/src/micronaut-jdbc/build/native/nativeCompile/micronaut-jdbc micronaut

EXPOSE 8080
ENV MICRONAUT_ENVIRONMENTS=benchmark
ENTRYPOINT "./micronaut"
