cmake_minimum_required(VERSION 3.20)
project(zeroshm_examples LANGUAGES C CXX)

set(CMAKE_CXX_STANDARD 23)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

find_package(Threads REQUIRED)

# Include the ZeroSHM header-only library
include_directories(${CMAKE_SOURCE_DIR}/../../cpp/include)

# Link xxhash library (required by ZeroSHM)
add_library(libxxhash STATIC ${CMAKE_SOURCE_DIR}/../../cpp/include/zeroshm/detail/xxhash.c)
target_include_directories(libxxhash PUBLIC
    ${CMAKE_SOURCE_DIR}/../../cpp/include)

# Link rt on Linux
if(UNIX AND NOT APPLE)
    set(POSIX_LIBS rt)
else()
    set(POSIX_LIBS "")
endif()

# Producer
add_executable(producer producer.cpp)
target_link_libraries(producer Threads::Threads libxxhash ${POSIX_LIBS})

# Consumer
add_executable(consumer consumer.cpp)
target_link_libraries(consumer Threads::Threads libxxhash ${POSIX_LIBS})

