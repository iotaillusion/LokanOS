cmake_minimum_required(VERSION 3.16)
project(lokan_c_sdk VERSION 0.1.0 LANGUAGES C)

set(CMAKE_C_STANDARD 99)
set(CMAKE_C_STANDARD_REQUIRED ON)
set(CMAKE_POSITION_INDEPENDENT_CODE ON)

find_package(CURL REQUIRED)

add_library(lokan SHARED src/lokan.c)
add_library(lokan_static STATIC src/lokan.c)
set_target_properties(lokan_static PROPERTIES OUTPUT_NAME lokan)
set_target_properties(lokan PROPERTIES VERSION ${PROJECT_VERSION} SOVERSION 0)

target_include_directories(lokan
    PUBLIC
        $<BUILD_INTERFACE:${CMAKE_CURRENT_SOURCE_DIR}/include>
        $<INSTALL_INTERFACE:include>)

target_include_directories(lokan_static
    PUBLIC
        $<BUILD_INTERFACE:${CMAKE_CURRENT_SOURCE_DIR}/include>
        $<INSTALL_INTERFACE:include>)

target_link_libraries(lokan PUBLIC CURL::libcurl)
target_link_libraries(lokan_static PUBLIC CURL::libcurl)

add_executable(lokan_health_example examples/health.c)
target_link_libraries(lokan_health_example PRIVATE lokan)
target_include_directories(lokan_health_example PRIVATE ${CMAKE_CURRENT_SOURCE_DIR}/include)
set_target_properties(lokan_health_example PROPERTIES INSTALL_RPATH "\$ORIGIN/../lib")

install(TARGETS lokan lokan_static
        EXPORT lokanTargets
        ARCHIVE DESTINATION lib
        LIBRARY DESTINATION lib
        RUNTIME DESTINATION bin
        INCLUDES DESTINATION include)

install(TARGETS lokan_health_example RUNTIME DESTINATION bin)

install(DIRECTORY include/ DESTINATION include)

install(EXPORT lokanTargets
        NAMESPACE lokan::
        DESTINATION lib/cmake/lokan)
