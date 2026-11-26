cmake_minimum_required(VERSION 3.20)
project(zeroshm VERSION 2.2.0 LANGUAGES C CXX)

set(CMAKE_CXX_STANDARD 23)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_C_STANDARD 11)
set(CMAKE_C_STANDARD_REQUIRED ON)

# Include directories
include_directories(${CMAKE_CURRENT_SOURCE_DIR}/include)

# Find packages
find_package(Threads REQUIRED)

# Create static library for xxhash (C implementation)
add_library(libxxhash STATIC include/zeroshm/detail/xxhash.c)
target_include_directories(
  libxxhash
  PUBLIC
  $<BUILD_INTERFACE:${CMAKE_CURRENT_SOURCE_DIR}/include>
  $<INSTALL_INTERFACE:include>
)

# Create header-only library
add_library(libzeroshm INTERFACE)
target_include_directories(
  libzeroshm
  INTERFACE
  $<BUILD_INTERFACE:${CMAKE_CURRENT_SOURCE_DIR}/include>
  $<INSTALL_INTERFACE:include>
)
target_link_libraries(libzeroshm INTERFACE Threads::Threads libxxhash)

# Link rt library on Linux/Unix systems
if(UNIX AND NOT APPLE)
  target_link_libraries(libzeroshm INTERFACE rt)
endif()

# POSIX libraries: rt on Linux, empty on macOS (included in libSystem)
if(UNIX AND NOT APPLE)
  set(ZEROSHM_POSIX_LIBS rt)
else()
  set(ZEROSHM_POSIX_LIBS "")
endif()

# Set up installation
install(
  TARGETS libzeroshm libxxhash
  EXPORT zeroshmTargets
  LIBRARY
  DESTINATION lib
  ARCHIVE
  DESTINATION lib
  RUNTIME
  DESTINATION bin
  INCLUDES
  DESTINATION include
)

install(
  EXPORT zeroshmTargets
  FILE zeroshmTargets.cmake
  NAMESPACE zeroshm::
  DESTINATION lib/cmake/zeroshm
)

include(CMakePackageConfigHelpers)
write_basic_package_version_file(
  "zeroshmConfigVersion.cmake"
  VERSION ${PROJECT_VERSION}
  COMPATIBILITY SameMajorVersion
)

configure_package_config_file(
  "${CMAKE_CURRENT_SOURCE_DIR}/zeroshmConfig.cmake.in"
  "${CMAKE_CURRENT_BINARY_DIR}/zeroshmConfig.cmake"
  INSTALL_DESTINATION lib/cmake/zeroshm
)

install(
  FILES
  "${CMAKE_CURRENT_BINARY_DIR}/zeroshmConfig.cmake"
  "${CMAKE_CURRENT_BINARY_DIR}/zeroshmConfigVersion.cmake"
  DESTINATION lib/cmake/zeroshm
)

# Enable testing
enable_testing()

# Add subdirectories
add_subdirectory(benchmarks)

# Google Test - try to find system installation (e.g., Homebrew on macOS)
# If not found, fall back to FetchContent from GitHub
set(GTEST_FOUND FALSE)

# Check common Homebrew locations on macOS
if(APPLE)
  if(EXISTS "/opt/homebrew/lib/cmake/GTest")
    list(APPEND CMAKE_PREFIX_PATH "/opt/homebrew")
  elseif(EXISTS "/usr/local/lib/cmake/GTest")
    list(APPEND CMAKE_PREFIX_PATH "/usr/local")
  endif()
endif()

find_package(GTest QUIET CONFIG)
if(GTest_FOUND)
  message(STATUS "Found system GTest: ${GTest_DIR}")
  set(GTEST_FOUND TRUE)
endif()

# Also try module mode (FindGTest.cmake)
if(NOT GTEST_FOUND)
  find_package(GTest QUIET MODULE)
  if(GTest_FOUND)
    message(STATUS "Found system GTest (module mode)")
    set(GTEST_FOUND TRUE)
  endif()
endif()

if(NOT GTEST_FOUND)
  message(STATUS "System GTest not found, using FetchContent")
  include(FetchContent)
  FetchContent_Declare(
    googletest
    GIT_REPOSITORY https://github.com/google/googletest.git
    GIT_TAG v1.14.0
  )
  FetchContent_MakeAvailable(googletest)
endif()

# Create alias targets for consistent usage regardless of how GTest was found.
# System GTest (CONFIG mode) defines GTest::gtest / GTest::gtest_main.
# FetchContent defines gtest / gtest_main.
if(TARGET GTest::gtest AND NOT TARGET gtest)
  add_library(gtest ALIAS GTest::gtest)
endif()
if(TARGET GTest::gtest_main AND NOT TARGET gtest_main)
  add_library(gtest_main ALIAS GTest::gtest_main)
endif()
if(TARGET GTest::gmock AND NOT TARGET gmock)
  add_library(gmock ALIAS GTest::gmock)
endif()
if(TARGET GTest::gmock_main AND NOT TARGET gmock_main)
  add_library(gmock_main ALIAS GTest::gmock_main)
endif()

# Test executables
add_executable(test_table tests/test_table.cpp)
target_link_libraries(test_table libzeroshm gtest_main Threads::Threads)

add_executable(test_memory tests/test_memory.cpp)
target_link_libraries(
  test_memory libzeroshm gtest_main Threads::Threads ${ZEROSHM_POSIX_LIBS}
)

add_executable(test_array tests/test_array.cpp)
target_link_libraries(
  test_array libzeroshm gtest_main Threads::Threads ${ZEROSHM_POSIX_LIBS}
)

add_executable(test_queue tests/test_queue.cpp)
target_link_libraries(
  test_queue libzeroshm gtest_main Threads::Threads ${ZEROSHM_POSIX_LIBS}
)

add_executable(test_stack tests/test_stack.cpp)
target_link_libraries(
  test_stack libzeroshm gtest_main Threads::Threads ${ZEROSHM_POSIX_LIBS}
)

add_executable(
  test_lockfree_comprehensive tests/test_lockfree_comprehensive.cpp
)
target_link_libraries(
  test_lockfree_comprehensive
  libzeroshm
  gtest_main
  Threads::Threads
  ${ZEROSHM_POSIX_LIBS}
)

add_executable(test_stress tests/test_stress.cpp)
target_link_libraries(
  test_stress libzeroshm gtest_main Threads::Threads ${ZEROSHM_POSIX_LIBS}
)

add_executable(test_edge_cases tests/test_edge_cases.cpp)
target_link_libraries(
  test_edge_cases libzeroshm gtest_main Threads::Threads ${ZEROSHM_POSIX_LIBS}
)

add_executable(test_aba_problem tests/test_aba_problem.cpp)
target_link_libraries(
  test_aba_problem libzeroshm gtest_main Threads::Threads ${ZEROSHM_POSIX_LIBS}
)

add_executable(test_table_stress tests/test_table_stress.cpp)
target_link_libraries(
  test_table_stress libzeroshm gtest_main Threads::Threads ${ZEROSHM_POSIX_LIBS}
)

add_executable(test_memory_boundaries tests/test_memory_boundaries.cpp)
target_link_libraries(
  test_memory_boundaries
  libzeroshm
  gtest_main
  Threads::Threads
  ${ZEROSHM_POSIX_LIBS}
)

add_executable(test_failure_recovery tests/test_failure_recovery.cpp)
target_link_libraries(
  test_failure_recovery
  libzeroshm
  gtest_main
  Threads::Threads
  ${ZEROSHM_POSIX_LIBS}
)

add_executable(test_crash_safety tests/test_crash_safety.cpp)
target_link_libraries(
  test_crash_safety libzeroshm gtest_main Threads::Threads ${ZEROSHM_POSIX_LIBS}
)

add_executable(test_new_structures tests/test_new_structures.cpp)
target_link_libraries(
  test_new_structures
  libzeroshm
  gtest_main
  Threads::Threads
  ${ZEROSHM_POSIX_LIBS}
)

add_executable(test_codata tests/test_codata.cpp)
target_link_libraries(
  test_codata libzeroshm gtest_main Threads::Threads ${ZEROSHM_POSIX_LIBS}
)

add_executable(test_cli_tool tests/test_cli_tool.cpp)
target_link_libraries(
  test_cli_tool libzeroshm gtest_main Threads::Threads ${ZEROSHM_POSIX_LIBS}
)

add_executable(test_coverage_improvement tests/test_coverage_improvement.cpp)
target_link_libraries(
  test_coverage_improvement
  libzeroshm
  gtest_main
  Threads::Threads
  ${ZEROSHM_POSIX_LIBS}
)

add_executable(test_semaphore tests/test_semaphore.cpp)
target_link_libraries(
  test_semaphore libzeroshm gtest_main Threads::Threads ${ZEROSHM_POSIX_LIBS}
)

add_executable(test_semaphore_optimized tests/test_semaphore_optimized.cpp)
target_link_libraries(
  test_semaphore_optimized
  libzeroshm
  gtest_main
  Threads::Threads
  ${ZEROSHM_POSIX_LIBS}
)

add_executable(test_barrier tests/test_barrier.cpp)
target_link_libraries(
  test_barrier libzeroshm gtest_main Threads::Threads ${ZEROSHM_POSIX_LIBS}
)

add_executable(test_latch tests/test_latch.cpp)
target_link_libraries(
  test_latch libzeroshm gtest_main Threads::Threads ${ZEROSHM_POSIX_LIBS}
)

add_executable(test_once tests/test_once.cpp)
target_link_libraries(
  test_once libzeroshm gtest_main Threads::Threads ${ZEROSHM_POSIX_LIBS}
)

add_executable(test_event tests/test_event.cpp)
target_link_libraries(
  test_event libzeroshm gtest_main Threads::Threads ${ZEROSHM_POSIX_LIBS}
)

add_executable(test_mutex tests/test_mutex.cpp)
target_link_libraries(
  test_mutex libzeroshm gtest_main Threads::Threads ${ZEROSHM_POSIX_LIBS}
)

add_executable(test_monitor tests/test_monitor.cpp)
target_link_libraries(
  test_monitor libzeroshm gtest_main Threads::Threads ${ZEROSHM_POSIX_LIBS}
)

add_executable(test_rwlock tests/test_rwlock.cpp)
target_link_libraries(
  test_rwlock libzeroshm gtest_main Threads::Threads ${ZEROSHM_POSIX_LIBS}
)

add_executable(test_signal tests/test_signal.cpp)
target_link_libraries(
  test_signal libzeroshm gtest_main Threads::Threads ${ZEROSHM_POSIX_LIBS}
)

add_executable(test_long_names tests/test_long_names.cpp)
target_link_libraries(
  test_long_names libzeroshm gtest_main Threads::Threads ${ZEROSHM_POSIX_LIBS}
)

add_executable(test_hash_collision tests/test_hash_collision.cpp)
target_link_libraries(
  test_hash_collision
  libzeroshm
  gtest_main
  Threads::Threads
  ${ZEROSHM_POSIX_LIBS}
)

add_executable(test_error tests/test_error.cpp)
target_link_libraries(
  test_error libzeroshm gtest_main Threads::Threads ${ZEROSHM_POSIX_LIBS}
)

add_executable(test_layout tests/test_layout.cpp)
target_link_libraries(
  test_layout libzeroshm gtest_main Threads::Threads ${ZEROSHM_POSIX_LIBS}
)

add_executable(test_latest tests/test_latest.cpp)
target_link_libraries(
  test_latest libzeroshm gtest_main Threads::Threads ${ZEROSHM_POSIX_LIBS}
)

add_executable(test_latest_performance tests/test_latest_performance.cpp)
target_link_libraries(
  test_latest_performance
  libzeroshm
  gtest_main
  Threads::Threads
  ${ZEROSHM_POSIX_LIBS}
)

add_executable(test_c_interop tests/test_c_interop.cpp)
target_link_libraries(
  test_c_interop libzeroshm Threads::Threads ${ZEROSHM_POSIX_LIBS}
)

# Add tests with proper categorization
# FAST tests: <100ms, core functionality
add_test(NAME table_test COMMAND test_table)
set_tests_properties(table_test PROPERTIES LABELS "fast;unit" TIMEOUT 5)

add_test(NAME memory_test COMMAND test_memory)
set_tests_properties(memory_test PROPERTIES LABELS "fast;unit" TIMEOUT 5)

add_test(NAME array_test COMMAND test_array)
set_tests_properties(
  array_test
  PROPERTIES
  LABELS "fast;unit;lockfree"
  TIMEOUT
  5
)

add_test(NAME queue_test COMMAND test_queue)
set_tests_properties(
  queue_test
  PROPERTIES
  LABELS "fast;unit;lockfree"
  TIMEOUT
  5
)

add_test(NAME stack_test COMMAND test_stack)
set_tests_properties(
  stack_test
  PROPERTIES
  LABELS "fast;unit;lockfree"
  TIMEOUT
  5
)

add_test(NAME edge_cases_test COMMAND test_edge_cases)
set_tests_properties(edge_cases_test PROPERTIES LABELS "fast;unit" TIMEOUT 5)

add_test(NAME new_structures_test COMMAND test_new_structures)
set_tests_properties(
  new_structures_test
  PROPERTIES
  LABELS "fast;unit"
  TIMEOUT
  10
)

# MEDIUM tests: <5s, multi-threaded with minimal delays
add_test(NAME lockfree_comprehensive_test COMMAND test_lockfree_comprehensive)
set_tests_properties(
  lockfree_comprehensive_test
  PROPERTIES
  LABELS "medium;integration;lockfree"
  TIMEOUT
  30
)

add_test(NAME aba_problem_test COMMAND test_aba_problem)
set_tests_properties(
  aba_problem_test
  PROPERTIES
  LABELS "medium;integration;lockfree"
  TIMEOUT
  10
)

add_test(NAME memory_boundaries_test COMMAND test_memory_boundaries)
set_tests_properties(
  memory_boundaries_test
  PROPERTIES
  LABELS "medium;integration"
  TIMEOUT
  10
)

add_test(NAME failure_recovery_test COMMAND test_failure_recovery)
set_tests_properties(
  failure_recovery_test
  PROPERTIES
  LABELS "medium;integration"
  TIMEOUT
  15
)

add_test(NAME crash_safety_test COMMAND test_crash_safety)
set_tests_properties(
  crash_safety_test
  PROPERTIES
  LABELS "fast;unit;lockfree"
  TIMEOUT
  10
)

add_test(NAME codata_test COMMAND test_codata)
set_tests_properties(codata_test PROPERTIES LABELS "medium;unit" TIMEOUT 10)

add_test(NAME cli_tool_test COMMAND test_cli_tool)
set_tests_properties(
  cli_tool_test
  PROPERTIES
  LABELS "medium;integration"
  TIMEOUT
  10
)

add_test(NAME coverage_improvement_test COMMAND test_coverage_improvement)
set_tests_properties(
  coverage_improvement_test
  PROPERTIES
  LABELS "medium;unit"
  TIMEOUT
  10
)

add_test(NAME latch_test COMMAND test_latch)
set_tests_properties(latch_test PROPERTIES LABELS "medium;unit;sync" TIMEOUT 10)

add_test(NAME once_test COMMAND test_once)
set_tests_properties(once_test PROPERTIES LABELS "fast;unit;sync" TIMEOUT 5)

add_test(NAME event_test COMMAND test_event)
set_tests_properties(event_test PROPERTIES LABELS "fast;unit;sync" TIMEOUT 5)

add_test(NAME mutex_test COMMAND test_mutex)
set_tests_properties(mutex_test PROPERTIES LABELS "fast;unit;sync" TIMEOUT 5)

add_test(NAME monitor_test COMMAND test_monitor)
set_tests_properties(
  monitor_test
  PROPERTIES
  LABELS "medium;unit;sync"
  TIMEOUT
  10
)

add_test(NAME rwlock_test COMMAND test_rwlock)
set_tests_properties(
  rwlock_test
  PROPERTIES
  LABELS "medium;unit;sync"
  TIMEOUT
  10
)

add_test(NAME signal_test COMMAND test_signal)
set_tests_properties(signal_test PROPERTIES LABELS "fast;unit" TIMEOUT 5)

add_test(NAME long_names_test COMMAND test_long_names)
set_tests_properties(long_names_test PROPERTIES LABELS "fast;unit" TIMEOUT 5)

add_test(NAME hash_collision_test COMMAND test_hash_collision)
set_tests_properties(
  hash_collision_test
  PROPERTIES
  LABELS "fast;unit"
  TIMEOUT
  5
)

add_test(NAME error_test COMMAND test_error)
set_tests_properties(error_test PROPERTIES LABELS "fast;unit" TIMEOUT 5)

add_test(NAME layout_test COMMAND test_layout)
set_tests_properties(layout_test PROPERTIES LABELS "fast;unit" TIMEOUT 5)

add_test(NAME latest_test COMMAND test_latest)
set_tests_properties(
  latest_test
  PROPERTIES
  LABELS "medium;unit;lockfree"
  TIMEOUT
  30
)

add_test(NAME latest_performance_test COMMAND test_latest_performance)
set_tests_properties(
  latest_performance_test
  PROPERTIES
  LABELS "slow;performance"
  TIMEOUT
  300
  DISABLED
  TRUE
)

# SLOW tests: >5s, full synchronization tests with threading delays
add_test(NAME semaphore_test COMMAND test_semaphore)
set_tests_properties(
  semaphore_test
  PROPERTIES
  LABELS "slow;integration;sync"
  TIMEOUT
  600
  DISABLED
  FALSE
)

add_test(NAME semaphore_optimized_test COMMAND test_semaphore_optimized)
set_tests_properties(
  semaphore_optimized_test
  PROPERTIES
  LABELS "fast;unit;sync"
  TIMEOUT
  30
  DISABLED
  FALSE
)

add_test(NAME barrier_test COMMAND test_barrier)
set_tests_properties(
  barrier_test
  PROPERTIES
  LABELS "slow;integration;sync"
  TIMEOUT
  600
  DISABLED
  FALSE
)

# STRESS tests: >30s, exhaustive stress testing (disabled by default)
add_test(NAME stress_test COMMAND test_stress)
set_tests_properties(
  stress_test
  PROPERTIES
  LABELS "stress;integration;lockfree"
  TIMEOUT
  1800
  DISABLED
  TRUE
)

add_test(NAME table_stress_test COMMAND test_table_stress)
set_tests_properties(
  table_stress_test
  PROPERTIES
  LABELS "stress;integration"
  TIMEOUT
  1800
  DISABLED
  TRUE
)

# CLI Tools
add_executable(zeroshm tools/zeroshm.cpp)
target_link_libraries(zeroshm libzeroshm Threads::Threads ${ZEROSHM_POSIX_LIBS})

# Install headers
install(DIRECTORY include/zeroshm DESTINATION include)

# Install CLI tools
install(TARGETS zeroshm RUNTIME DESTINATION bin)

# Custom test targets for convenient execution
add_custom_target(
  test_fast
  COMMAND ${CMAKE_CTEST_COMMAND} -L fast --output-on-failure
  COMMENT "Running fast tests only (~30 seconds)"
)

add_custom_target(
  test_default
  COMMAND ${CMAKE_CTEST_COMMAND} -L "fast|medium" --output-on-failure
  COMMENT "Running default test suite (fast + medium, ~2 minutes)"
)

add_custom_target(
  test_ci
  COMMAND ${CMAKE_CTEST_COMMAND} -LE stress --output-on-failure
  COMMENT "Running CI test suite (all except stress tests, ~10 minutes)"
)

add_custom_target(
  test_all
  COMMAND ${CMAKE_CTEST_COMMAND} --output-on-failure
  COMMENT "Running complete test suite including stress tests (~30 minutes)"
)

