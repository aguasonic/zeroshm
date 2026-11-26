#include <string>
#include <system_error>

#include <gtest/gtest.h>

#include <zeroshm/error.h>

using namespace zeroshm;

// 
// error_message() tests
// 

TEST(ErrorTest, ErrorMessageSuccess) {
   EXPECT_EQ(error_message(Error::Success), "Success");
}

TEST(ErrorTest, ErrorMessageTableErrors) {
   EXPECT_EQ(error_message(Error::NotFound), "Entry not found");
   EXPECT_EQ(error_message(Error::TableFull), "Table is full");
   EXPECT_EQ(error_message(Error::NameTooLong),
             "Name too long (max 254 characters)");
   EXPECT_EQ(error_message(Error::NameAlreadyExists), "Name already exists");
}

TEST(ErrorTest, ErrorMessageMemoryErrors) {
   EXPECT_EQ(error_message(Error::AllocationOverflow),
             "Allocation size overflow");
   EXPECT_EQ(error_message(Error::BoundsExceeded),
             "Allocation would exceed memory bounds");
}

TEST(ErrorTest, ErrorMessageQueueErrors) {
   EXPECT_EQ(error_message(Error::QueueFull), "Queue is full");
   EXPECT_EQ(error_message(Error::QueueEmpty), "Queue is empty");
}

TEST(ErrorTest, ErrorMessageTypeErrors) {
   EXPECT_EQ(error_message(Error::TypeMismatch), "Type size mismatch");
   EXPECT_EQ(error_message(Error::InvalidCapacity), "Invalid capacity");
}

TEST(ErrorTest, ErrorMessageStackErrors) {
   EXPECT_EQ(error_message(Error::StackFull), "Stack is full");
   EXPECT_EQ(error_message(Error::StackUnderflow), "Stack is empty");
}

TEST(ErrorTest, ErrorMessageMapErrors) {
   EXPECT_EQ(error_message(Error::MapFull), "Map is full");
   EXPECT_EQ(error_message(Error::MapNotFound), "Key not found in map");
}

TEST(ErrorTest, ErrorMessageSetErrors) {
   EXPECT_EQ(error_message(Error::SetFull), "Set is full");
   EXPECT_EQ(error_message(Error::SetNotFound), "Element not found in set");
}

TEST(ErrorTest, ErrorMessagePoolErrors) {
   EXPECT_EQ(error_message(Error::PoolExhausted), "Pool is exhausted");
   EXPECT_EQ(error_message(Error::PoolInvalidPointer),
             "Invalid pointer to deallocate");
}

TEST(ErrorTest, ErrorMessageRingErrors) {
   EXPECT_EQ(error_message(Error::RingFull), "Ring buffer is full");
   EXPECT_EQ(error_message(Error::RingEmpty), "Ring buffer is empty");
}

TEST(ErrorTest, ErrorMessageSyncErrors) {
   EXPECT_EQ(error_message(Error::SemaphoreNotAvailable),
             "Semaphore not available");
   EXPECT_EQ(error_message(Error::LatchNotComplete), "Latch not complete");
   EXPECT_EQ(error_message(Error::EventNotSignaled), "Event not signaled");
   EXPECT_EQ(error_message(Error::LockNotAcquired), "Lock not acquired");
}

TEST(ErrorTest, ErrorMessageCodataErrors) {
   EXPECT_EQ(error_message(Error::FutureNotReady), "Future not ready");
   EXPECT_EQ(error_message(Error::LazyNotReady), "Lazy value not yet computed");
}

TEST(ErrorTest, ErrorMessageChannelErrors) {
   EXPECT_EQ(error_message(Error::ChannelFull), "Channel is full");
   EXPECT_EQ(error_message(Error::ChannelEmpty), "Channel is empty");
   EXPECT_EQ(error_message(Error::ChannelClosed), "Channel is closed");
}

TEST(ErrorTest, ErrorMessageSignalErrors) {
   EXPECT_EQ(error_message(Error::SignalNotChanged),
             "Signal not changed within timeout");
}

TEST(ErrorTest, ErrorMessageGeneralErrors) {
   EXPECT_EQ(error_message(Error::InvalidArgument), "Invalid argument");
}

TEST(ErrorTest, ErrorMessageUnknown) {
   // Cast an invalid integer to Error to hit the default case
   Error unknown = static_cast<Error>(999);
   EXPECT_EQ(error_message(unknown), "Unknown error");
}

TEST(ErrorTest, ErrorMessageAllCodesNonEmpty) {
   // Verify every defined error code has a non-empty message
   for (int i = 0; i <= 31; i++) {
      Error err = static_cast<Error>(i);
      std::string_view msg = error_message(err);
      EXPECT_FALSE(msg.empty()) << "Error code " << i << " has empty message";
   }
}

// 
// make_error_code() tests
// 

TEST(ErrorTest, MakeErrorCodeCategory) {
   std::error_code ec = make_error_code(Error::QueueFull);
   EXPECT_EQ(ec.category().name(), "zeroshm");
}

TEST(ErrorTest, MakeErrorCodeValue) {
   std::error_code ec_full = make_error_code(Error::QueueFull);
   std::error_code ec_empty = make_error_code(Error::QueueEmpty);

   EXPECT_EQ(ec_full.value(), static_cast<int>(Error::QueueFull));
   EXPECT_EQ(ec_empty.value(), static_cast<int>(Error::QueueEmpty));
   EXPECT_NE(ec_full, ec_empty);
}

TEST(ErrorTest, MakeErrorCodeMessage) {
   std::error_code ec = make_error_code(Error::NotFound);
   EXPECT_EQ(ec.message(), "Entry not found");

   ec = make_error_code(Error::TableFull);
   EXPECT_EQ(ec.message(), "Table is full");

   ec = make_error_code(Error::InvalidArgument);
   EXPECT_EQ(ec.message(), "Invalid argument");
}

TEST(ErrorTest, MakeErrorCodeSuccess) {
   std::error_code ec = make_error_code(Error::Success);
   EXPECT_FALSE(ec);   // Success should compare equal to default error_code
   EXPECT_EQ(ec.value(), 0);
}

TEST(ErrorTest, MakeErrorCodeDistinctCategories) {
   std::error_code ec_zeroshm = make_error_code(Error::QueueFull);
   std::error_code ec_generic =
       std::make_error_code(std::errc::invalid_argument);

   EXPECT_NE(ec_zeroshm.category(), ec_generic.category());
   EXPECT_EQ(ec_zeroshm.category().name(), "zeroshm");
   EXPECT_NE(ec_generic.category().name(), "zeroshm");
}

// 
// std::is_error_code_enum trait
// 

TEST(ErrorTest, IsErrorCodeEnum) {
   static_assert(std::is_error_code_enum<Error>::value,
                 "Error must specialize std::is_error_code_enum");
   EXPECT_TRUE(std::is_error_code_enum<Error>::value);
}

TEST(ErrorTest, AdlMakeErrorCodeWorks) {
   // Unqualified make_error_code should find zeroshm::make_error_code via ADL
   std::error_code ec = make_error_code(Error::PoolExhausted);
   EXPECT_EQ(ec.category().name(), "zeroshm");
   EXPECT_EQ(ec.message(), "Pool is exhausted");
}
