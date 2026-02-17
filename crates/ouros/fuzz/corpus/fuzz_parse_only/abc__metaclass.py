from abc import ABC, abstractmethod


class Animal(ABC):
    @abstractmethod
    def speak(self):
        pass


class Dog(Animal):
    def speak(self):
        return 'Woof'


dog = Dog()
assert dog.speak() == 'Woof', 'concrete method works'

try:
    animal = Animal()
    assert False, 'should not be able to instantiate ABC'
except TypeError as e:
    assert 'abstract' in str(e).lower(), f'wrong error: {e}'

print('abc test passed')
